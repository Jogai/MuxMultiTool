//! Batch operations end to end over the Connect router, as the dev
//! frontend uses it (JSON): the dry run with every file's output name,
//! streams and command, one job per file, outputs that exist being skipped
//! unless overwriting is allowed, rules resolved per file with a collision
//! and a container refusal skipping only the file concerned, and the
//! request validation. Needs ffmpeg and ffprobe on `PATH`.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner};
use muxmultitool_server::{Config, app};
use serde_json::{Value, json};
use tower::ServiceExt;

const PLAN: &str = "/rpc/muxmultitool.batch.v1.BatchService/PlanBatch";
const SUBMIT: &str = "/rpc/muxmultitool.batch.v1.BatchService/SubmitBatch";
const GET: &str = "/rpc/muxmultitool.jobs.v1.JobsService/GetJob";
const LIST_FOLDER: &str = "/rpc/muxmultitool.files.v1.FilesService/ListFolder";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with an `in` folder holding a two-second MKV (H.264, AAC
/// in English, SubRip), two MP4s (H.264, AAC) and a third MP4 in a
/// subfolder, plus an empty `out` folder:
///
/// ```text
/// root/
///   in/
///     clip.mkv, clip.mp4, second.mp4
///     sub/third.mp4
///   out/
/// ```
struct MediaRoot {
	dir: tempfile::TempDir,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(60));
		let input = dir.path().join("in");
		std::fs::create_dir_all(input.join("sub"))?;
		std::fs::create_dir(dir.path().join("out"))?;
		let subs = dir.path().join("subs.srt");
		std::fs::write(&subs, "1\n00:00:00,000 --> 00:00:01,000\nhello\n\n")?;
		let mkv = input.join("clip.mkv");
		let cmd = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("testsrc2=duration=2:size=64x64:rate=10")
			.option("-f", "lavfi")
			.input("sine=frequency=440:duration=2")
			.input(&subs)
			.args(["-map", "0", "-map", "1", "-map", "2"])
			.option("-c:v", "libx264")
			.option("-preset", "ultrafast")
			.option("-pix_fmt", "yuv420p")
			.option("-c:a", "aac")
			.option("-c:s", "srt")
			.option("-metadata:s:a:0", "language=eng")
			.output(&mkv);
		runner.run(&cmd, &limits, None).await?;
		let mp4 = input.join("clip.mp4");
		let cmd = FfmpegCommand::new()
			.input(&mkv)
			.args(["-map", "0:0", "-map", "0:1"])
			.option("-c", "copy")
			.output(&mp4);
		runner.run(&cmd, &limits, None).await?;
		std::fs::copy(&mp4, input.join("second.mp4"))?;
		std::fs::copy(&mp4, input.join("sub/third.mp4"))?;
		std::fs::remove_file(subs)?;
		Ok(Self { dir })
	}

	fn path(&self) -> &Path {
		self.dir.path()
	}

	fn input(&self) -> PathBuf {
		self.path().join("in")
	}

	fn out(&self) -> PathBuf {
		self.path().join("out")
	}

	fn config(&self) -> Config {
		Config {
			address: IpAddr::V4(Ipv4Addr::LOCALHOST),
			port: 0,
			media_roots: vec![self.path().to_path_buf()],
			frontend_dir: self.path().join("no-frontend"),
			..Config::default()
		}
	}

	async fn probe(&self, relative: &str) -> Result<Vec<(CodecType, String)>> {
		let probe = Runner::default()
			.probe(
				&self.path().join(relative),
				&Limits::timeout(Duration::from_secs(30)),
			)
			.await?;
		Ok(probe
			.streams
			.iter()
			.map(|s| (s.codec_type, s.codec_name.clone().unwrap_or_default()))
			.collect())
	}
}

/// Sends a JSON request to the in-process router and returns the status
/// and the decoded body.
async fn post_json(router: &axum::Router, path: &str, body: Value) -> Result<(StatusCode, Value)> {
	let request = Request::post(path)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_string()))?;
	let response = router.clone().oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes();
	Ok((parts.status, serde_json::from_slice(&body)?))
}

/// Waits for every job in `jobs` to end and returns them.
async fn wait_for(router: &axum::Router, jobs: &Value) -> Result<Vec<Value>> {
	let mut done = Vec::new();
	for job in jobs.as_array().into_iter().flatten() {
		let id = job["id"].as_str().unwrap_or_default().to_owned();
		let job = tokio::time::timeout(Duration::from_secs(60), async {
			loop {
				let (_, json) = post_json(router, GET, json!({ "jobId": id })).await?;
				let state = json["job"]["state"].as_str().unwrap_or_default();
				if state != "JOB_STATE_QUEUED" && state != "JOB_STATE_RUNNING" {
					return Ok::<_, Box<dyn std::error::Error>>(json["job"].clone());
				}
				tokio::time::sleep(Duration::from_millis(50)).await;
			}
		})
		.await??;
		done.push(job);
	}
	Ok(done)
}

fn names(items: &Value, field: &str) -> Vec<String> {
	items
		.as_array()
		.into_iter()
		.flatten()
		.map(|i| i[field].as_str().unwrap_or_default().to_owned())
		.collect()
}

#[tokio::test]
async fn every_mp4_becomes_an_mkv_in_the_output_folder_as_its_own_job() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let operation = json!({
		"folder": root.input(),
		"patterns": ["*.mp4"],
		"outputFolder": root.out(),
		"template": "{name}.mkv",
		"mode": "SELECTION_MODE_ALL",
	});

	// The dry run: both files, named after their source, every stream kept.
	let (status, plan) = post_json(&router, PLAN, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{plan}");
	assert_eq!(plan["ready"], 2, "{plan}");
	assert_eq!(
		names(&plan["items"], "outputName"),
		["clip.mkv", "second.mkv"]
	);
	let item = &plan["items"][0];
	assert_eq!(item["input"]["name"], "clip.mp4");
	assert_eq!(item["input"]["path"], "in/clip.mp4");
	assert_eq!(
		item["output"],
		root.out().join("clip.mkv").display().to_string()
	);
	assert_eq!(item["ready"], true);
	assert_eq!(item["container"], "MKV");
	assert!(item["skip"].as_str().is_none_or(str::is_empty), "{item}");
	assert!(item["overwrites"].as_bool().is_none_or(|o| !o), "{item}");
	let streams = item["streams"].as_array().unwrap();
	assert_eq!(streams.len(), 2);
	assert_eq!(streams[0]["stream"]["type"], "STREAM_TYPE_VIDEO");
	assert_eq!(streams[0]["kept"], true);
	assert_eq!(streams[0]["why"], "all streams");
	assert_eq!(streams[1]["stream"]["language"], "eng");
	let command = item["command"].as_str().unwrap();
	let expected = format!(
		"-i {} -map 0 -c copy {}",
		root.input().join("clip.mp4").display(),
		root.out().join("clip.mkv").display()
	);
	assert!(command.ends_with(&expected), "{command}");
	let duration = item["durationMs"].as_str().unwrap().parse::<i64>().unwrap();
	assert!((1500..=2500).contains(&duration), "{duration}");
	assert_eq!(plan["date"].as_str().unwrap().len(), 10);

	// The submit queues one job per file with the same commands.
	let (status, submitted) = post_json(&router, SUBMIT, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{submitted}");
	assert_eq!(
		names(&submitted["jobs"], "name"),
		["clip.mkv", "second.mkv"]
	);
	assert_eq!(submitted["jobs"][0]["command"], item["command"]);
	assert!(
		submitted["skipped"].as_array().is_none_or(Vec::is_empty),
		"{submitted}"
	);
	let jobs = wait_for(&router, &submitted["jobs"]).await.unwrap();
	assert!(
		jobs.iter().all(|j| j["state"] == "JOB_STATE_SUCCEEDED"),
		"{jobs:?}"
	);
	for name in ["out/clip.mkv", "out/second.mkv"] {
		assert_eq!(
			root.probe(name).await.unwrap(),
			[
				(CodecType::Video, "h264".into()),
				(CodecType::Audio, "aac".into())
			]
		);
	}
}

#[tokio::test]
async fn existing_outputs_are_skipped_unless_overwriting_is_allowed() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let operation = json!({
		"folder": root.input(),
		"patterns": ["*.mp4"],
		"outputFolder": root.out(),
		"template": "{name}.mkv",
	});
	// Something already sits where both outputs would go.
	for name in ["clip.mkv", "second.mkv"] {
		std::fs::write(root.out().join(name), b"old").unwrap();
	}

	// The outputs exist: both files are skipped, nothing is written.
	let (status, plan) = post_json(&router, PLAN, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{plan}");
	assert!(plan["ready"].as_u64().is_none_or(|r| r == 0), "{plan}");
	assert_eq!(
		plan["items"][0]["skip"],
		"the output exists; allow overwriting to replace it"
	);
	assert_eq!(plan["items"][0]["ready"].as_bool(), None);
	let (status, submitted) = post_json(&router, SUBMIT, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{submitted}");
	assert!(submitted["jobs"].as_array().is_none_or(Vec::is_empty));
	assert_eq!(
		names(&submitted["skipped"], "outputName"),
		["clip.mkv", "second.mkv"]
	);

	// Allowed explicitly, they are replaced: the jobs run ffmpeg with -y.
	let mut overwrite = operation.clone();
	overwrite["overwrite"] = json!(true);
	let (status, plan) = post_json(&router, PLAN, json!({ "operation": overwrite }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{plan}");
	assert_eq!(plan["ready"], 2);
	assert_eq!(plan["items"][0]["overwrites"], true);
	assert!(
		plan["items"][0]["command"]
			.as_str()
			.unwrap()
			.contains(" -y -i ")
	);
	let (status, submitted) = post_json(&router, SUBMIT, json!({ "operation": overwrite }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{submitted}");
	let jobs = wait_for(&router, &submitted["jobs"]).await.unwrap();
	assert_eq!(jobs.len(), 2);
	assert!(
		jobs.iter().all(|j| j["state"] == "JOB_STATE_SUCCEEDED"),
		"{jobs:?}"
	);
	assert_ne!(std::fs::read(root.out().join("clip.mkv")).unwrap(), b"old");
}

#[tokio::test]
async fn rules_resolve_per_file_and_one_file_being_skipped_stops_nothing() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	// The folder as an id from the file browser, searched recursively; the
	// outputs go next to their inputs.
	let (_, roots) = post_json(&router, LIST_FOLDER, json!({})).await.unwrap();
	let (_, listing) = post_json(
		&router,
		LIST_FOLDER,
		json!({ "folderId": roots["entries"][0]["id"] }),
	)
	.await
	.unwrap();
	let folder = listing["entries"]
		.as_array()
		.unwrap()
		.iter()
		.find(|e| e["name"] == "in")
		.unwrap()["id"]
		.clone();
	let operation = json!({
		"folder": folder,
		"patterns": ["*"],
		"recursive": true,
		"template": "{name}.{lang}.mp4",
		"mode": "SELECTION_MODE_RULES",
		"rules": { "rules": [
			{ "action": "RULE_ACTION_DROP", "streamType": "STREAM_TYPE_SUBTITLE", "codec": "subrip" }
		] },
	});
	let (status, plan) = post_json(&router, PLAN, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{plan}");
	let items = plan["items"].as_array().unwrap();
	assert_eq!(
		names(&plan["items"], "outputName"),
		[
			"clip.eng.mp4",
			"clip.eng.mp4",
			"second.eng.mp4",
			"third.eng.mp4"
		]
	);
	// The MKV's subtitle is dropped by the rule, so MP4 can hold the rest.
	let mkv = &items[0];
	assert_eq!(mkv["input"]["name"], "clip.mkv");
	assert_eq!(mkv["ready"], true);
	assert_eq!(
		mkv["output"],
		root.input().join("clip.eng.mp4").display().to_string()
	);
	let whys: Vec<&str> = mkv["streams"]
		.as_array()
		.unwrap()
		.iter()
		.map(|s| s["why"].as_str().unwrap())
		.collect();
	assert_eq!(
		whys,
		[
			"no rule applies",
			"no rule applies",
			"rule: drop subtitle subrip"
		]
	);
	assert_eq!(mkv["streams"][2]["kept"].as_bool(), None);
	// clip.mp4 renders to the same name as clip.mkv: the first one keeps it.
	let collided = &items[1];
	assert_eq!(collided["input"]["name"], "clip.mp4");
	assert_eq!(collided["ready"].as_bool(), None);
	assert_eq!(
		collided["skip"],
		format!(
			"another file of the batch has the same output name: {}",
			root.input().join("clip.mkv").display()
		)
	);
	assert_eq!(
		items[3]["output"],
		root.input().join("sub/third.eng.mp4").display().to_string()
	);
	assert_eq!(plan["ready"], 3);

	let (status, submitted) = post_json(&router, SUBMIT, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{submitted}");
	assert_eq!(
		names(&submitted["jobs"], "name"),
		["clip.eng.mp4", "second.eng.mp4", "third.eng.mp4"]
	);
	assert_eq!(names(&submitted["skipped"], "outputName"), ["clip.eng.mp4"]);
	let jobs = wait_for(&router, &submitted["jobs"]).await.unwrap();
	assert!(
		jobs.iter().all(|j| j["state"] == "JOB_STATE_SUCCEEDED"),
		"{jobs:?}"
	);
	assert_eq!(
		root.probe("in/clip.eng.mp4").await.unwrap(),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Audio, "aac".into())
		]
	);
	assert!(root.input().join("sub/third.eng.mp4").is_file());
}

#[tokio::test]
async fn a_container_refusal_skips_only_the_file_concerned() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	// `{name}.{ext}` keeps each file's own container, so both run.
	let operation = json!({
		"folder": root.input(),
		"patterns": ["clip.m??"],
		"outputFolder": root.out(),
		"template": "{n}-{parent}-{name}.{ext}",
		"mode": "SELECTION_MODE_ALL",
	});
	let (status, plan) = post_json(&router, PLAN, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{plan}");
	assert_eq!(
		names(&plan["items"], "outputName"),
		["1-in-clip.mkv", "2-in-clip.mp4"]
	);
	assert_eq!(plan["ready"], 2);

	// Into MP4, the MKV's SubRip subtitle blocks: that file is skipped with
	// the reason, the MP4 still runs.
	let operation = json!({
		"folder": root.input(),
		"patterns": ["clip.m??"],
		"outputFolder": root.out(),
		"template": "{name}.mp4",
		"mode": "SELECTION_MODE_ALL",
	});
	let (status, plan) = post_json(&router, PLAN, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{plan}");
	assert_eq!(plan["ready"], 1);
	assert_eq!(
		plan["items"][0]["skip"],
		"the MP4 output cannot hold every selected stream: MP4 cannot hold subrip subtitles (stream 2); drop stream 2 or choose MKV"
	);
	assert_eq!(plan["items"][1]["ready"], true);
}

#[tokio::test]
async fn requests_are_validated_and_outputs_stay_inside_the_roots() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let outside = tempfile::tempdir().unwrap();
	let good = json!({
		"folder": root.input(),
		"patterns": ["*.mp4"],
		"outputFolder": root.out(),
		"template": "{name}.mkv",
	});
	let refused = |patch: Value| {
		let mut operation = good.clone();
		for (key, value) in patch.as_object().unwrap() {
			operation[key] = value.clone();
		}
		operation
	};
	let cases = [
		(json!({ "patterns": [] }), "invalid_argument"),
		(json!({ "template": "" }), "invalid_argument"),
		(json!({ "template": "sub/{name}.mkv" }), "invalid_argument"),
		(json!({ "template": "{title}.mkv" }), "invalid_argument"),
		(json!({ "folder": "" }), "invalid_argument"),
		(json!({ "folder": "relative" }), "invalid_argument"),
		(
			json!({ "folder": root.input().join("clip.mp4") }),
			"invalid_argument",
		),
		(
			json!({ "folder": root.path().join("missing") }),
			"not_found",
		),
		(json!({ "folder": outside.path() }), "permission_denied"),
		(
			json!({ "outputFolder": outside.path() }),
			"permission_denied",
		),
		(
			json!({ "outputFolder": root.input().join("clip.mkv") }),
			"invalid_argument",
		),
		(
			json!({ "mode": "SELECTION_MODE_RULES" }),
			"invalid_argument",
		),
		(
			json!({ "mode": "SELECTION_MODE_RULES", "rules": { "rules": [{ "action": "RULE_ACTION_DROP", "language": "" }] } }),
			"invalid_argument",
		),
	];
	for (patch, code) in cases {
		for rpc in [PLAN, SUBMIT] {
			let (status, body) =
				post_json(&router, rpc, json!({ "operation": refused(patch.clone()) }))
					.await
					.unwrap();
			assert_ne!(status, StatusCode::OK, "{patch} on {rpc}: {body}");
			assert_eq!(body["code"], code, "{patch} on {rpc}: {body}");
		}
	}

	// No match is an empty plan, but nothing to submit.
	let none = refused(json!({ "patterns": ["*.avi"] }));
	let (status, plan) = post_json(&router, PLAN, json!({ "operation": none }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{plan}");
	assert!(plan["items"].as_array().is_none_or(Vec::is_empty), "{plan}");
	let (status, body) = post_json(&router, SUBMIT, json!({ "operation": none }))
		.await
		.unwrap();
	assert_ne!(status, StatusCode::OK);
	assert_eq!(body["code"], "failed_precondition", "{body}");

	let (status, body) = post_json(&router, PLAN, json!({})).await.unwrap();
	assert_ne!(status, StatusCode::OK);
	assert_eq!(body["code"], "invalid_argument", "{body}");

	// Nothing was written anywhere.
	assert!(std::fs::read_dir(root.out()).unwrap().next().is_none());
	assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
}
