//! Remux and selective stream copy end to end over the Connect router, as
//! the dev frontend uses it (JSON): the preview with its streams, issues
//! and command, a remux job, keep/drop and video-only jobs with their
//! outputs probed, a submit refused for a container that cannot hold a
//! stream, a rule-based selection with its "why" per stream and its
//! warnings, an input given as a file id, and the request validation.
//! Needs ffmpeg and ffprobe on `PATH`.

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

const SUBMIT: &str = "/rpc/muxmultitool.jobs.v1.JobsService/SubmitJob";
const GET: &str = "/rpc/muxmultitool.jobs.v1.JobsService/GetJob";
const PREVIEW: &str = "/rpc/muxmultitool.streams.v1.StreamsService/PreviewStreams";
const LIST_FOLDER: &str = "/rpc/muxmultitool.files.v1.FilesService/ListFolder";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with a two-second MKV (H.264, AAC, SubRip) and an MP4
/// (H.264, AAC) in it, plus an `out` folder.
struct MediaRoot {
	dir: tempfile::TempDir,
	mkv: PathBuf,
	mp4: PathBuf,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(60));
		let subs = dir.path().join("subs.srt");
		std::fs::write(&subs, "1\n00:00:00,000 --> 00:00:01,000\nhello\n\n")?;
		let mkv = dir.path().join("clip.mkv");
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
		let mp4 = dir.path().join("clip.mp4");
		let cmd = FfmpegCommand::new()
			.input(&mkv)
			.args(["-map", "0:0", "-map", "0:1"])
			.option("-c", "copy")
			.output(&mp4);
		runner.run(&cmd, &limits, None).await?;
		std::fs::create_dir(dir.path().join("out"))?;
		Ok(Self { dir, mkv, mp4 })
	}

	fn path(&self) -> &Path {
		self.dir.path()
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

	async fn probe(&self, name: &str) -> Result<Vec<(CodecType, String)>> {
		let probe = Runner::default()
			.probe(
				&self.path().join(name),
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

/// Submits `operation` as a `select_streams` job and waits for it to end.
async fn run_job(router: &axum::Router, operation: Value) -> Result<Value> {
	let (status, json) = post_json(router, SUBMIT, json!({ "selectStreams": operation })).await?;
	if status != StatusCode::OK {
		return Err(format!("submit refused: {json}").into());
	}
	let id = json["job"]["id"].as_str().unwrap_or_default().to_owned();
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
	Ok(job)
}

fn kept_indexes(preview: &Value) -> Vec<u64> {
	preview["streams"]
		.as_array()
		.into_iter()
		.flatten()
		.filter(|s| s["kept"] == true)
		.map(|s| s["stream"]["index"].as_u64().unwrap_or_default())
		.collect()
}

#[tokio::test]
async fn a_remux_is_previewed_and_run() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let operation =
		json!({ "input": root.mp4, "output": "remuxed.mkv", "mode": "SELECTION_MODE_ALL" });

	let (status, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{preview}");
	assert_eq!(preview["container"], "MKV");
	assert_eq!(preview["outputName"], "remuxed.mkv");
	assert_eq!(preview["canRun"], true);
	// Proto3 JSON leaves default values out: no issues, no field.
	assert!(
		preview["issues"].as_array().is_none_or(Vec::is_empty),
		"{preview}"
	);
	assert_eq!(kept_indexes(&preview), [0, 1]);
	let streams = preview["streams"].as_array().unwrap();
	assert_eq!(streams[0]["stream"]["type"], "STREAM_TYPE_VIDEO");
	assert_eq!(streams[0]["stream"]["codec"], "h264");
	assert_eq!(streams[0]["why"], "all streams");
	assert_eq!(streams[1]["stream"]["codec"], "aac");
	assert_eq!(streams[1]["stream"]["language"], "eng");
	let command = preview["command"].as_str().unwrap();
	let expected = format!(
		"-i {} -map 0 -c copy {}",
		root.mp4.display(),
		root.path().join("remuxed.mkv").display()
	);
	assert!(command.ends_with(&expected), "{command}");
	let argv = preview["argv"].as_array().unwrap();
	assert_eq!(argv[argv.len() - 3], "-c");
	assert_eq!(argv[argv.len() - 2], "copy");

	// The job runs the same command and the output has both streams.
	let job = run_job(&router, operation).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(job["name"], "remuxed.mkv");
	assert_eq!(job["command"], preview["command"]);
	assert_eq!(
		root.probe("remuxed.mkv").await.unwrap(),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Audio, "aac".into())
		]
	);
}

#[tokio::test]
async fn keep_drop_and_video_only_write_exactly_the_chosen_streams() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	// Keep, with an absolute output path in another folder.
	let job = run_job(
		&router,
		json!({
			"input": root.mkv,
			"output": root.path().join("out/kept.mkv"),
			"mode": "SELECTION_MODE_KEEP",
			"streamIndexes": [0, 2],
		}),
	)
	.await
	.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(
		root.probe("out/kept.mkv").await.unwrap(),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Subtitle, "subrip".into())
		]
	);

	// Drop the audio.
	let job = run_job(
		&router,
		json!({
			"input": root.mkv,
			"output": "dropped.mkv",
			"mode": "SELECTION_MODE_DROP",
			"streamIndexes": [1],
		}),
	)
	.await
	.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(
		root.probe("dropped.mkv").await.unwrap(),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Subtitle, "subrip".into())
		]
	);

	// Video only, as an MP4: the subtitle that MP4 cannot hold is dropped,
	// so there is no issue.
	let operation =
		json!({ "input": root.mkv, "output": "video.mp4", "mode": "SELECTION_MODE_VIDEO_ONLY" });
	let (_, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(preview["canRun"], true, "{preview}");
	assert_eq!(kept_indexes(&preview), [0]);
	let streams = preview["streams"].as_array().unwrap();
	assert_eq!(streams[0]["why"], "video");
	assert_eq!(streams[1]["why"], "not video");
	assert_eq!(streams[2]["why"], "not video");
	let job = run_job(&router, operation).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(
		root.probe("video.mp4").await.unwrap(),
		[(CodecType::Video, "h264".into())]
	);
}

#[tokio::test]
async fn rules_resolve_to_a_selection_with_the_rule_as_the_why() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	// Keep English audio only and drop SubRip subtitles: the video has no
	// language tag and no rule covers it.
	let rules = json!({
		"rules": [
			{ "action": "RULE_ACTION_KEEP", "streamType": "STREAM_TYPE_AUDIO", "language": "ENG" },
			{ "action": "RULE_ACTION_DROP", "streamType": "STREAM_TYPE_SUBTITLE", "codec": "subrip" },
		],
	});
	let operation = json!({
		"input": root.mkv,
		"output": "rules.mkv",
		"mode": "SELECTION_MODE_RULES",
		"rules": rules,
	});
	let (status, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{preview}");
	assert_eq!(kept_indexes(&preview), [0, 1]);
	let streams = preview["streams"].as_array().unwrap();
	assert_eq!(streams[0]["why"], "no rule applies");
	assert_eq!(streams[1]["why"], "rule: keep audio ENG");
	assert_eq!(streams[2]["why"], "rule: drop subtitle subrip");
	assert_eq!(preview["canRun"], true);
	assert!(preview.get("warnings").is_none(), "{preview}");
	let job = run_job(&router, operation).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(
		root.probe("rules.mkv").await.unwrap(),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Audio, "aac".into())
		]
	);

	// Dropping the only audio stream warns but runs; a keep-only rule for
	// a language the file lacks drops the English audio and, with untagged
	// streams dropped, the untagged video as well.
	let operation = json!({
		"input": root.mkv,
		"output": "no-audio.mkv",
		"mode": "SELECTION_MODE_RULES",
		"rules": { "rules": [{ "action": "RULE_ACTION_DROP", "streamType": "STREAM_TYPE_AUDIO", "language": "eng" }] },
	});
	let (_, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(kept_indexes(&preview), [0, 2]);
	assert_eq!(
		preview["warnings"],
		json!(["the rules drop every audio stream of the input"])
	);
	assert_eq!(preview["canRun"], true);
	let job = run_job(&router, operation).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");

	let (_, preview) = post_json(
		&router,
		PREVIEW,
		json!({ "operation": {
			"input": root.mkv,
			"output": "fre.mkv",
			"mode": "SELECTION_MODE_RULES",
			"rules": {
				"rules": [{ "action": "RULE_ACTION_KEEP", "language": "fre" }],
				"untagged": "UNTAGGED_LANGUAGE_DROP",
			},
		} }),
	)
	.await
	.unwrap();
	assert_eq!(preview["code"], "invalid_argument", "{preview}");
	assert_eq!(preview["message"], "no stream is left to copy");
}

#[tokio::test]
async fn a_container_that_cannot_hold_a_stream_is_reported_before_running() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let operation =
		json!({ "input": root.mkv, "output": "clip-copy.mp4", "mode": "SELECTION_MODE_ALL" });

	let (status, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{preview}");
	assert_eq!(preview["container"], "MP4");
	// `false` is a default proto3 JSON leaves out.
	assert_ne!(preview["canRun"], true, "{preview}");
	let issues = preview["issues"].as_array().unwrap();
	assert_eq!(issues.len(), 1, "{preview}");
	assert_eq!(issues[0]["streamIndex"], 2);
	assert_eq!(issues[0]["blocking"], true);
	assert_eq!(
		issues[0]["message"],
		"MP4 cannot hold subrip subtitles (stream 2); drop stream 2 or choose MKV"
	);

	// The same operation cannot be submitted, and nothing is written.
	let (status, json) = post_json(&router, SUBMIT, json!({ "selectStreams": operation }))
		.await
		.unwrap();
	assert_ne!(status, StatusCode::OK);
	assert_eq!(json["code"], "failed_precondition", "{json}");
	assert_eq!(
		json["message"],
		"the MP4 output cannot hold every selected stream: MP4 cannot hold subrip subtitles (stream 2); drop stream 2 or choose MKV"
	);
	assert!(!root.path().join("clip-copy.mp4").exists());

	// Dropping the subtitle makes it runnable.
	let (_, preview) = post_json(
		&router,
		PREVIEW,
		json!({ "operation": { "input": root.mkv, "output": "clip-copy.mp4", "mode": "SELECTION_MODE_DROP", "streamIndexes": [2] } }),
	)
	.await
	.unwrap();
	assert_eq!(preview["canRun"], true, "{preview}");
	assert_eq!(kept_indexes(&preview), [0, 1]);
	assert_eq!(preview["streams"][2]["why"], "unticked");
}

#[tokio::test]
async fn the_input_can_be_a_file_id() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let (_, roots) = post_json(&router, LIST_FOLDER, json!({})).await.unwrap();
	let folder = roots["entries"][0]["id"].as_str().unwrap();
	let (_, listing) = post_json(&router, LIST_FOLDER, json!({ "folderId": folder }))
		.await
		.unwrap();
	let id = listing["entries"]
		.as_array()
		.unwrap()
		.iter()
		.find(|e| e["name"] == "clip.mkv")
		.unwrap()["id"]
		.as_str()
		.unwrap()
		.to_owned();

	let (status, preview) = post_json(
		&router,
		PREVIEW,
		json!({ "operation": { "input": id, "output": "by-id.mkv", "mode": "SELECTION_MODE_KEEP", "streamIndexes": [1] } }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{preview}");
	assert_eq!(kept_indexes(&preview), [1]);
	assert!(
		preview["command"]
			.as_str()
			.unwrap()
			.contains(&root.mkv.display().to_string()),
		"{preview}"
	);

	// A folder id is not an input.
	let (status, json) = post_json(
		&router,
		PREVIEW,
		json!({ "operation": { "input": folder, "output": "x.mkv" } }),
	)
	.await
	.unwrap();
	assert_ne!(status, StatusCode::OK);
	assert_eq!(json["code"], "invalid_argument", "{json}");
	assert!(
		json["message"].as_str().unwrap().contains("folder"),
		"{json}"
	);
}

#[tokio::test]
async fn rule_requests_are_validated() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let rules = |rules: Value| {
		json!({ "operation": {
			"input": root.mkv,
			"output": "x.mkv",
			"mode": "SELECTION_MODE_RULES",
			"rules": rules,
		} })
	};
	let cases = [
		(
			json!({ "operation": { "input": root.mkv, "output": "x.mkv", "mode": "SELECTION_MODE_RULES" } }),
			"mode RULES needs rules; use mode ALL for a remux",
		),
		(
			rules(json!({ "rules": [{ "language": "fre" }] })),
			"rule 0 needs an action: drop or keep",
		),
		(
			rules(json!({ "rules": [{ "action": "RULE_ACTION_DROP" }] })),
			"rule 0 needs a language, codec or stream index to match",
		),
		(
			rules(
				json!({ "rules": [{ "action": "RULE_ACTION_DROP", "codec": "aac" }, { "action": "RULE_ACTION_KEEP", "language": " " }] }),
			),
			"rule 1 has an empty value",
		),
	];
	for (body, message) in cases {
		let (status, json) = post_json(&router, PREVIEW, body.clone()).await.unwrap();
		assert_ne!(status, StatusCode::OK, "{body}");
		assert_eq!(json["code"], "invalid_argument", "{body}: {json}");
		assert_eq!(json["message"], message, "{body}: {json}");
	}
}

#[tokio::test]
async fn stream_requests_are_validated() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let op = |fields: Value| {
		let mut operation = json!({ "input": root.mkv, "output": "x.mkv" });
		for (key, value) in fields.as_object().unwrap() {
			operation[key] = value.clone();
		}
		operation
	};
	let cases = [
		(
			json!({}),
			PREVIEW,
			"invalid_argument",
			"an operation is required",
		),
		(
			json!({ "operation": op(json!({ "output": "x.avi" })) }),
			PREVIEW,
			"invalid_argument",
			"the output's extension picks the container; use one of .mkv, .mka, .mks, .mp4, .m4v, .m4a, .mov, .webm",
		),
		(
			json!({ "operation": op(json!({ "output": "" })) }),
			PREVIEW,
			"invalid_argument",
			"output path is required",
		),
		(
			json!({ "operation": op(json!({ "output": "sub/x.mkv" })) }),
			PREVIEW,
			"invalid_argument",
			"output must be an absolute path or a bare file name: sub/x.mkv",
		),
		(
			json!({ "operation": op(json!({ "mode": "SELECTION_MODE_KEEP", "streamIndexes": [7] })) }),
			PREVIEW,
			"invalid_argument",
			"the input has no stream 7",
		),
		(
			json!({ "operation": op(json!({ "mode": "SELECTION_MODE_KEEP" })) }),
			PREVIEW,
			"invalid_argument",
			"no stream is left to copy",
		),
		(
			json!({ "operation": op(json!({ "input": root.path().join("subs.srt"), "mode": "SELECTION_MODE_VIDEO_ONLY" })) }),
			PREVIEW,
			"invalid_argument",
			"the input has no video stream",
		),
		(
			json!({ "operation": op(json!({ "input": "/nowhere/clip.mkv" })) }),
			PREVIEW,
			"not_found",
			"input file not found: /nowhere/clip.mkv",
		),
		(
			json!({ "selectStreams": op(json!({ "output": "clip.mp4" })) }),
			SUBMIT,
			"already_exists",
			&format!("output file already exists: {}", root.mp4.display()),
		),
		(
			json!({ "selectStreams": op(json!({ "output": "/elsewhere/x.mkv" })) }),
			SUBMIT,
			"invalid_argument",
			"output folder does not exist: /elsewhere",
		),
	];
	for (body, path, code, message) in cases {
		let (status, json) = post_json(&router, path, body.clone()).await.unwrap();
		assert_ne!(status, StatusCode::OK, "{body}");
		assert_eq!(json["code"], code, "{body}: {json}");
		assert_eq!(json["message"], message, "{body}: {json}");
	}
}
