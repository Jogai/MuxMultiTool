//! The crop feature end to end over the Connect router, as the dev frontend
//! uses it (JSON): the plan, black-bar detection, a crop job whose output
//! shows the cropped size, a refusal for another codec, and the request
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
const PLAN: &str = "/rpc/muxmultitool.crop.v1.CropService/PlanCrop";
const DETECT: &str = "/rpc/muxmultitool.crop.v1.CropService/DetectBlackBars";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with a letterboxed H.264 clip (640×240 of picture in a
/// 640×360 frame, 60 px bars top and bottom), the same picture in MPEG-4
/// part 2, and an audio-only file.
struct MediaRoot {
	dir: tempfile::TempDir,
	h264: PathBuf,
	mpeg4: PathBuf,
	audio: PathBuf,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(60));
		let letterboxed = |encoder: &str, output: &Path| {
			let cmd = FfmpegCommand::new()
				.option("-f", "lavfi")
				.input("testsrc2=duration=3:size=640x240:rate=25")
				.option("-f", "lavfi")
				.input("sine=frequency=440:duration=3")
				.option("-vf", "pad=640:360:0:60:black")
				.option("-c:v", encoder);
			let cmd = if encoder == "libx264" {
				cmd.option("-preset", "ultrafast")
			} else {
				cmd
			};
			cmd.option("-pix_fmt", "yuv420p")
				.option("-c:a", "aac")
				.output(output)
		};
		let h264 = dir.path().join("bars.mp4");
		runner
			.run(&letterboxed("libx264", &h264), &limits, None)
			.await?;
		let mpeg4 = dir.path().join("other.mkv");
		runner
			.run(&letterboxed("mpeg4", &mpeg4), &limits, None)
			.await?;
		let audio = dir.path().join("tone.m4a");
		let cmd = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("sine=frequency=440:duration=1")
			.option("-c:a", "aac")
			.output(&audio);
		runner.run(&cmd, &limits, None).await?;
		Ok(Self {
			dir,
			h264,
			mpeg4,
			audio,
		})
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

	/// A crop operation on `input` with an odd top value that snaps.
	fn operation(&self, input: &Path, output: &str) -> serde_json::Value {
		serde_json::json!({
			"input": input,
			"output": self.path().join(output),
			"crop": { "top": 61, "bottom": 60 },
		})
	}
}

/// Sends a JSON request to the in-process router and returns the status
/// and the decoded body.
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

/// Submits `body` and waits for the job to end; returns the job, or the
/// error body when the submit was refused.
async fn run_job(router: &axum::Router, body: serde_json::Value) -> Result<serde_json::Value> {
	let (status, json) = post_json(router, SUBMIT, body).await?;
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

#[tokio::test]
async fn bars_are_detected_planned_and_cropped_without_re_encoding() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	// Detection suggests the bars, snapped, with the size they leave.
	let (status, json) = post_json(
		&router,
		DETECT,
		serde_json::json!({ "input": root.h264, "positionMs": 500 }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["crop"]["top"], 60, "{json}");
	assert_eq!(json["crop"]["bottom"], 60, "{json}");
	assert!(
		json["crop"].get("left").is_none(),
		"zero is omitted: {json}"
	);
	assert!(json["framesScanned"].as_u64().unwrap() >= 1, "{json}");
	assert_eq!(
		(json["width"].as_u64(), json["height"].as_u64()),
		(Some(640), Some(240))
	);

	// The plan: the odd value snaps, the result size and the limitations.
	let operation = root.operation(&root.h264, "bars.cropped.mp4");
	let (status, json) = post_json(&router, PLAN, serde_json::json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["support"]["supported"], true, "{json}");
	assert_eq!(json["source"]["codec"], "H.264", "{json}");
	assert_eq!(json["source"]["unitX"], 2, "{json}");
	assert_eq!(json["source"]["unitY"], 2, "{json}");
	assert_eq!(json["source"]["pixFmt"], "yuv420p", "{json}");
	assert_eq!(json["source"]["width"], 640, "{json}");
	assert_eq!(json["crop"]["top"], 60, "snapped from 61: {json}");
	assert_eq!(json["crop"]["bottom"], 60, "{json}");
	assert_eq!(
		(json["width"].as_u64(), json["height"].as_u64()),
		(Some(640), Some(240))
	);
	let limitations = json["limitations"].as_array().unwrap();
	assert_eq!(limitations.len(), 3, "{json}");
	assert!(
		limitations[1]
			.as_str()
			.unwrap()
			.contains("file size stays the same")
	);
	assert!(
		limitations[2]
			.as_str()
			.unwrap()
			.contains("ignore the crop window")
	);
	let command = json["command"].as_str().unwrap();
	// The header's own 8 lines of padding are added to the bottom.
	assert!(
		command.contains("h264_metadata=crop_left=0:crop_right=0:crop_top=60:crop_bottom=68"),
		"{command}"
	);
	assert!(command.contains("-c copy"), "{command}");
	assert!(command.contains("bars.cropped.mp4"), "{command}");

	// A plan without an output names the default one.
	let mut without_output = operation.clone();
	without_output["output"] = serde_json::Value::String(String::new());
	let (status, json) = post_json(
		&router,
		PLAN,
		serde_json::json!({ "operation": without_output }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert!(
		json["command"]
			.as_str()
			.unwrap()
			.contains("bars.cropped.mp4"),
		"{json}"
	);

	// The crop as a job: the output shows the cropped size and is as large
	// as the input.
	let job = run_job(&router, serde_json::json!({ "crop": operation }))
		.await
		.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(job["name"], "bars.cropped.mp4");
	assert_eq!(job["command"], command, "the plan showed the real command");
	let output = root.path().join("bars.cropped.mp4");
	let probe = Runner::default()
		.probe(&output, &Limits::timeout(Duration::from_secs(30)))
		.await
		.unwrap();
	let video = probe.streams_of(CodecType::Video).next().unwrap();
	assert_eq!(video.codec_name.as_deref(), Some("h264"));
	assert_eq!((video.width, video.height), (Some(640), Some(240)));
	assert_eq!(probe.streams_of(CodecType::Audio).count(), 1);
	let in_size = std::fs::metadata(&root.h264).unwrap().len();
	let out_size = std::fs::metadata(&output).unwrap().len();
	assert!(
		in_size.abs_diff(out_size) < in_size / 50,
		"the file size stays the same: {in_size} vs {out_size}"
	);
}

#[tokio::test]
async fn another_codec_is_refused_with_the_reason() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let reason = "lossless crop needs H.264 or H.265; this video is mpeg4";

	// The plan says so without an error, so the UI can disable the option.
	let operation = root.operation(&root.mpeg4, "other.cropped.mkv");
	let (status, json) = post_json(&router, PLAN, serde_json::json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert!(json["support"].get("supported").is_none(), "false: {json}");
	assert_eq!(json["support"]["reason"], reason, "{json}");
	assert!(json.get("source").is_none(), "{json}");
	assert!(json.get("command").is_none(), "{json}");

	// Detection and the submit refuse.
	let (status, json) = post_json(&router, DETECT, serde_json::json!({ "input": root.mpeg4 }))
		.await
		.unwrap();
	assert_ne!(status, StatusCode::OK);
	assert_eq!(json["code"], "failed_precondition", "{json}");
	assert_eq!(json["message"], reason, "{json}");
	let (status, json) = post_json(&router, SUBMIT, serde_json::json!({ "crop": operation }))
		.await
		.unwrap();
	assert_ne!(status, StatusCode::OK);
	assert_eq!(json["code"], "failed_precondition", "{json}");
	assert_eq!(json["message"], reason, "{json}");
	assert!(!root.path().join("other.cropped.mkv").exists());

	// A file without video is not supported either.
	let (status, json) = post_json(
		&router,
		PLAN,
		serde_json::json!({ "operation": root.operation(&root.audio, "tone.cropped.m4a") }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(
		json["support"]["reason"], "the file has no video stream",
		"{json}"
	);
}

#[tokio::test]
async fn crop_requests_are_validated() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let out = |name: &str| root.path().join(name);
	let cases = [
		(
			serde_json::json!({}),
			PLAN,
			"invalid_argument",
			"an operation is required",
		),
		(
			serde_json::json!({ "crop": { "input": root.h264, "output": out("x.mp4") } }),
			SUBMIT,
			"invalid_argument",
			"nothing to crop",
		),
		(
			serde_json::json!({ "crop": { "input": root.h264, "output": out("x.mp4"), "crop": { "top": 1, "left": 1 } } }),
			SUBMIT,
			"invalid_argument",
			"nothing to crop",
		),
		(
			serde_json::json!({ "crop": { "input": root.h264, "output": out("x.mp4"), "crop": { "top": 300, "bottom": 50 } } }),
			SUBMIT,
			"invalid_argument",
			"cropping 350 px from a 360 px high picture leaves less than 16 px",
		),
		(
			serde_json::json!({ "operation": { "input": root.h264, "crop": { "left": 640 } } }),
			PLAN,
			"invalid_argument",
			"cropping 640 px from a 640 px wide picture leaves less than 16 px",
		),
		(
			serde_json::json!({ "input": root.h264, "positionMs": -1 }),
			DETECT,
			"invalid_argument",
			"position_ms must not be negative",
		),
		(
			serde_json::json!({ "input": "bars.mp4" }),
			DETECT,
			"invalid_argument",
			"must be absolute",
		),
		(
			serde_json::json!({ "crop": { "input": root.h264, "output": root.h264, "crop": { "top": 60 } } }),
			SUBMIT,
			"already_exists",
			"output file already exists",
		),
		(
			serde_json::json!({ "crop": { "input": out("missing.mp4"), "output": out("x.mp4"), "crop": { "top": 60 } } }),
			SUBMIT,
			"not_found",
			"input file not found",
		),
	];
	for (body, path, code, message) in cases {
		let (status, json) = post_json(&router, path, body.clone()).await.unwrap();
		assert_ne!(status, StatusCode::OK, "{body}");
		assert_eq!(json["code"], code, "{body}: {json}");
		assert!(
			json["message"]
				.as_str()
				.unwrap_or_default()
				.contains(message),
			"{body}: {json}"
		);
	}
	assert!(!out("x.mp4").exists());

	// A zero crop is a valid plan, so the frontend can show the size while
	// the user types.
	let (status, json) = post_json(
		&router,
		PLAN,
		serde_json::json!({ "operation": { "input": root.h264 } }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["support"]["supported"], true, "{json}");
	assert_eq!(json["crop"], serde_json::json!({}), "all zero: {json}");
	assert_eq!(
		(json["width"].as_u64(), json["height"].as_u64()),
		(Some(640), Some(360))
	);
}
