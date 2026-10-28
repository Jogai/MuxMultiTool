//! The cut service and the cut job end to end over the Connect router, as
//! the dev frontend uses it (JSON): keyframe lookup, typed times, the plan,
//! a keyframe cut into separate files, a merged export, and the request
//! validation. Needs ffmpeg and ffprobe on `PATH`.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner};
use muxmultitool_server::{Config, app};
use tower::ServiceExt;

const SUBMIT: &str = "/rpc/muxmultitool.jobs.v1.JobsService/SubmitJob";
const GET: &str = "/rpc/muxmultitool.jobs.v1.JobsService/GetJob";
const KEYFRAMES: &str = "/rpc/muxmultitool.cut.v1.CutService/GetKeyframes";
const PARSE_TIME: &str = "/rpc/muxmultitool.cut.v1.CutService/ParseTime";
const PLAN: &str = "/rpc/muxmultitool.cut.v1.CutService/PlanCut";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with a six-second 320×240 H.264 (B-frames, a keyframe
/// every second) + AAC clip in it.
struct MediaRoot {
	dir: tempfile::TempDir,
	clip: PathBuf,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let clip = dir.path().join("clip.mp4");
		let cmd = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("testsrc2=duration=6:size=320x240:rate=25")
			.option("-f", "lavfi")
			.input("sine=frequency=440:duration=6")
			.option("-c:v", "libx264")
			.option("-preset", "veryfast")
			.option("-bf", "3")
			.option("-g", "25")
			.option("-keyint_min", "25")
			.option("-sc_threshold", "0")
			.option("-pix_fmt", "yuv420p")
			.option("-c:a", "aac")
			.output(&clip);
		Runner::default()
			.run(&cmd, &Limits::timeout(Duration::from_secs(60)), None)
			.await?;
		Ok(Self { dir, clip })
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

	/// A keyframe cut of two ranges into separate files.
	fn operation(&self, output: &str) -> serde_json::Value {
		serde_json::json!({
			"input": self.clip,
			"segments": [
				{ "startSeconds": 3.4, "endSeconds": 5.0 },
				{ "startSeconds": 1.3, "endSeconds": 2.5 },
			],
			"cutMode": "CUT_MODE_KEYFRAME",
			"export": "CUT_EXPORT_SEPARATE_FILES",
			"output": self.path().join(output),
		})
	}
}

async fn post_json(
	router: &axum::Router,
	path: &str,
	body: serde_json::Value,
) -> Result<(StatusCode, serde_json::Value)> {
	let request = Request::post(path)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_string()))?;
	let response = router.clone().oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes();
	Ok((parts.status, serde_json::from_slice(&body)?))
}

/// Submits `operation` as a cut job and waits for it to end.
async fn run_cut(router: &axum::Router, operation: serde_json::Value) -> Result<serde_json::Value> {
	let (status, json) = post_json(router, SUBMIT, serde_json::json!({ "cut": operation })).await?;
	if status != StatusCode::OK {
		return Err(format!("submit refused: {json}").into());
	}
	let id = json["job"]["id"].as_str().unwrap_or_default().to_owned();
	let job = tokio::time::timeout(Duration::from_secs(60), async {
		loop {
			let (_, json) = post_json(router, GET, serde_json::json!({ "jobId": id })).await?;
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

async fn video_length(path: &Path) -> Result<f64> {
	let probe = Runner::default()
		.probe(path, &Limits::timeout(Duration::from_secs(30)))
		.await?;
	Ok(probe
		.streams_of(CodecType::Video)
		.next()
		.and_then(|s| s.duration)
		.or(probe.format.duration)
		.unwrap_or(f64::NAN))
}

#[tokio::test]
async fn keyframes_and_typed_times_come_from_the_file() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	let (status, json) = post_json(
		&router,
		KEYFRAMES,
		serde_json::json!({ "fileId": root.clip, "fromSeconds": 1.5, "toSeconds": 4.2 }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	let times: Vec<f64> = json["keyframesSeconds"]
		.as_array()
		.unwrap()
		.iter()
		.map(|t| (t.as_f64().unwrap() * 1000.0).round() / 1000.0)
		.filter(|t| *t >= 1.5)
		.collect();
	assert_eq!(times, [2.0, 3.0, 4.0], "{json}");
	assert_eq!(json["streamIndex"].as_u64().unwrap_or(0), 0, "{json}");
	assert!(
		(json["durationSeconds"].as_f64().unwrap() - 6.0).abs() < 0.1,
		"{json}"
	);

	// The whole file when the range is left out.
	let (status, json) = post_json(
		&router,
		KEYFRAMES,
		serde_json::json!({ "fileId": root.clip }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(
		json["keyframesSeconds"].as_array().unwrap().len(),
		6,
		"{json}"
	);

	// Typed times, checked against the file.
	let (status, json) = post_json(
		&router,
		PARSE_TIME,
		serde_json::json!({ "text": "0:00:02,5", "fileId": root.clip }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["seconds"], 2.5, "{json}");
	assert_eq!(json["timecode"], "0:00:02.500", "{json}");

	// Frames use the file's rate: 1 s and 5 frames at 25 fps.
	let (status, json) = post_json(
		&router,
		PARSE_TIME,
		serde_json::json!({ "text": "0:0:1:5", "fileId": root.clip }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["timecode"], "0:00:01.200", "{json}");

	// Without a file: no duration check, no frames.
	let (status, json) = post_json(&router, PARSE_TIME, serde_json::json!({ "text": "1h" }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["seconds"], 3600.0, "{json}");

	for (text, file) in [("9", true), ("abc", false), ("", false), ("0:0:1:5", false)] {
		let body = if file {
			serde_json::json!({ "text": text, "fileId": root.clip })
		} else {
			serde_json::json!({ "text": text })
		};
		let (status, json) = post_json(&router, PARSE_TIME, body).await.unwrap();
		assert_eq!(status, StatusCode::BAD_REQUEST, "{text}: {json}");
		assert_eq!(json["code"], "invalid_argument", "{text}: {json}");
	}
}

#[tokio::test]
async fn a_cut_is_planned_and_exported() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	let (status, json) = post_json(
		&router,
		PLAN,
		serde_json::json!({ "operation": root.operation("cut.mp4") }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	let segments = json["segments"].as_array().unwrap();
	assert_eq!(segments.len(), 2, "{json}");
	assert_eq!(segments[0]["number"], 1);
	assert_eq!(segments[0]["startSeconds"], 1.3);
	assert_eq!(segments[0]["cutStartSeconds"], 1.0, "{json}");
	assert_eq!(segments[1]["cutStartSeconds"], 3.0, "{json}");
	let outputs = json["outputs"].as_array().unwrap();
	assert_eq!(
		outputs,
		&[
			serde_json::json!(root.path().join("cut-1.mp4")),
			serde_json::json!(root.path().join("cut-2.mp4"))
		],
		"{json}"
	);
	let command = json["command"].as_str().unwrap();
	assert!(command.contains("-copyts"), "{command}");
	assert!(command.contains("-c copy"), "{command}");
	assert_eq!(json["concatList"].as_str().unwrap_or(""), "", "{json}");
	assert!(
		(json["totalDurationSeconds"].as_f64().unwrap() - 3.5).abs() < 0.001,
		"{json}"
	);

	let job = run_cut(&router, root.operation("cut.mp4")).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(job["name"], "cut-1.mp4");
	assert_eq!(job["command"], command, "the plan showed the real command");
	assert_eq!(job["progress"]["percent"], 100.0, "{job}");
	// 1.0 to 2.5 and 3.0 to 5.0: the ranges plus the lead-in to the keyframe.
	let first = video_length(&root.path().join("cut-1.mp4")).await.unwrap();
	let second = video_length(&root.path().join("cut-2.mp4")).await.unwrap();
	assert!((first - 1.5).abs() < 0.2, "{first}");
	assert!((second - 2.0).abs() < 0.2, "{second}");

	// The same ranges merged, in normal mode: each piece starts at the
	// keyframe after its start.
	let mut merged = root.operation("merged.mp4");
	merged["export"] = "CUT_EXPORT_MERGED".into();
	merged["cutMode"] = "CUT_MODE_NORMAL".into();
	let (status, json) = post_json(&router, PLAN, serde_json::json!({ "operation": merged }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["segments"][0]["cutStartSeconds"], 2.0, "{json}");
	assert_eq!(json["segments"][1]["cutStartSeconds"], 4.0, "{json}");
	assert_eq!(
		json["outputs"],
		serde_json::json!([root.path().join("merged.mp4")])
	);
	let list = json["concatList"].as_str().unwrap();
	assert!(
		list.contains("inpoint 2.05\noutpoint 2.5\nduration 0.5"),
		"{list}"
	);
	let job = run_cut(&router, merged).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	let total = video_length(&root.path().join("merged.mp4")).await.unwrap();
	assert!((total - 1.5).abs() < 0.3, "{total}");

	// Running it again would overwrite: refused.
	let (status, json) = post_json(
		&router,
		SUBMIT,
		serde_json::json!({ "cut": root.operation("cut.mp4") }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::CONFLICT, "{json}");
	assert_eq!(json["code"], "already_exists", "{json}");
}

#[tokio::test]
async fn bad_cut_requests_are_refused() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let refused = |operation: serde_json::Value, status: StatusCode, code: &'static str| {
		let router = router.clone();
		async move {
			let (got, json) =
				post_json(&router, PLAN, serde_json::json!({ "operation": operation }))
					.await
					.unwrap();
			assert_eq!(got, status, "{json}");
			assert_eq!(json["code"], code, "{json}");
			json["message"].as_str().unwrap_or_default().to_owned()
		}
	};

	let mut empty = root.operation("x.mp4");
	empty["segments"] = serde_json::json!([{ "startSeconds": 7, "endSeconds": 9 }]);
	let message = refused(empty, StatusCode::BAD_REQUEST, "invalid_argument").await;
	assert!(message.contains("no range"), "{message}");

	let mut outside = root.operation("x.mp4");
	outside["input"] = "/etc/hostname".into();
	refused(outside, StatusCode::FORBIDDEN, "permission_denied").await;

	let mut missing = root.operation("x.mp4");
	missing["input"] = serde_json::json!(root.path().join("nope.mp4"));
	refused(missing, StatusCode::NOT_FOUND, "not_found").await;

	let mut no_output = root.operation("x.mp4");
	no_output["output"] = "".into();
	refused(no_output, StatusCode::BAD_REQUEST, "invalid_argument").await;

	let mut bad_mode = root.operation("x.mp4");
	bad_mode["cutMode"] = 7.into();
	refused(bad_mode, StatusCode::BAD_REQUEST, "invalid_argument").await;

	let mut negative = root.operation("x.mp4");
	negative["segments"] = serde_json::json!([{ "startSeconds": -1, "endSeconds": 2 }]);
	refused(negative, StatusCode::BAD_REQUEST, "invalid_argument").await;

	let (status, json) = post_json(&router, PLAN, serde_json::json!({}))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}
