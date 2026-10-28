//! The GIF operations end to end over the Connect router, as the dev
//! frontend uses it (JSON): the estimate, a video → GIF job within its
//! limits, GIF → MP4 and `WebM` jobs, a submit refused over a limit, and the
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
use tower::ServiceExt;

const SUBMIT: &str = "/rpc/muxmultitool.jobs.v1.JobsService/SubmitJob";
const GET: &str = "/rpc/muxmultitool.jobs.v1.JobsService/GetJob";
const ESTIMATE: &str = "/rpc/muxmultitool.gif.v1.GifService/EstimateGif";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with a three-second 320×240 H.264 + AAC clip and an
/// audio-only file in it.
struct MediaRoot {
	dir: tempfile::TempDir,
	clip: PathBuf,
	audio: PathBuf,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(60));
		let clip = dir.path().join("clip.mp4");
		let cmd = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("testsrc2=duration=3:size=320x240:rate=25")
			.option("-f", "lavfi")
			.input("sine=frequency=440:duration=3")
			.option("-c:v", "libx264")
			.option("-preset", "ultrafast")
			.option("-pix_fmt", "yuv420p")
			.option("-c:a", "aac")
			.output(&clip);
		runner.run(&cmd, &limits, None).await?;
		let audio = dir.path().join("tone.m4a");
		let cmd = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("sine=frequency=440:duration=1")
			.option("-c:a", "aac")
			.output(&audio);
		runner.run(&cmd, &limits, None).await?;
		Ok(Self { dir, clip, audio })
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

	/// A video → GIF operation for the whole clip, 10 fps, 160 px wide.
	fn video_to_gif(&self, output: &str) -> serde_json::Value {
		serde_json::json!({
			"input": self.clip,
			"output": self.path().join(output),
			"startMs": 500,
			"endMs": 2500,
			"fps": 10,
			"width": 160,
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
async fn a_gif_is_estimated_made_and_converted_back() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	// The estimate before running: the picture keeps the aspect ratio.
	let (status, json) = post_json(
		&router,
		ESTIMATE,
		serde_json::json!({ "operation": root.video_to_gif("clip.gif") }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["estimate"]["width"], 160, "{json}");
	assert_eq!(json["estimate"]["height"], 120, "{json}");
	assert_eq!(json["estimate"]["fps"], 10, "{json}");
	assert_eq!(json["estimate"]["frames"], 20, "{json}");
	assert_eq!(json["estimate"]["durationMs"], "2000", "{json}");
	assert!(
		json["estimate"]["estimatedBytes"]
			.as_str()
			.and_then(|b| b.parse::<u64>().ok())
			.is_some_and(|b| b > 0),
		"{json}"
	);
	assert!(
		json.get("violations").is_none(),
		"within the limits: {json}"
	);
	let command = json["command"].as_str().unwrap();
	assert!(command.contains("palettegen"), "{command}");
	assert!(command.contains("clip.gif"), "{command}");

	// Video → GIF as a job.
	let job = run_job(
		&router,
		serde_json::json!({ "videoToGif": root.video_to_gif("clip.gif") }),
	)
	.await
	.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(job["name"], "clip.gif");
	assert_eq!(
		job["command"], command,
		"the estimate showed the real command"
	);
	assert_eq!(job["progress"]["percent"], 100.0, "{job}");
	let gif = root.path().join("clip.gif");
	let runner = Runner::default();
	let probe = runner
		.probe(&gif, &Limits::timeout(Duration::from_secs(30)))
		.await
		.unwrap();
	assert_eq!(probe.format.format_name, "gif");
	let video = probe.streams_of(CodecType::Video).next().unwrap();
	assert_eq!((video.width, video.height), (Some(160), Some(120)));
	assert_eq!(video.nb_frames, Some(20));

	// GIF → MP4 and GIF → WebM as jobs.
	for (format, output, container) in [
		("GIF_VIDEO_FORMAT_MP4", "back.mp4", "mp4"),
		("GIF_VIDEO_FORMAT_WEBM", "back.webm", "webm"),
	] {
		let job = run_job(
			&router,
			serde_json::json!({ "gifToVideo": { "input": gif, "output": root.path().join(output), "format": format } }),
		)
		.await
		.unwrap();
		assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
		let probe = runner
			.probe(
				&root.path().join(output),
				&Limits::timeout(Duration::from_secs(30)),
			)
			.await
			.unwrap();
		assert!(
			probe.format.format_name.contains(container),
			"{}",
			probe.format.format_name
		);
		let video = probe.streams_of(CodecType::Video).next().unwrap();
		assert_eq!((video.width, video.height), (Some(160), Some(120)));
		let codec = video.codec_name.as_deref().unwrap_or_default();
		match container {
			"mp4" => assert_eq!(codec, "h264"),
			_ => assert!(codec == "vp9" || codec == "vp8", "{codec}"),
		}
		assert_eq!(probe.streams_of(CodecType::Audio).count(), 0);
	}
}

#[tokio::test]
async fn a_gif_over_a_limit_is_refused_with_what_to_lower() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let mut operation = root.video_to_gif("big.gif");
	operation["limits"] =
		serde_json::json!({ "maxDurationMs": 1000, "maxWidth": 100, "maxBytes": 10_000 });

	let (status, json) = post_json(
		&router,
		ESTIMATE,
		serde_json::json!({ "operation": operation }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	let violations = json["violations"].as_array().unwrap();
	let limits: Vec<&str> = violations
		.iter()
		.map(|v| v["limit"].as_str().unwrap())
		.collect();
	assert_eq!(
		limits,
		["GIF_LIMIT_DURATION", "GIF_LIMIT_WIDTH", "GIF_LIMIT_SIZE"]
	);
	assert_eq!(
		violations[0]["message"],
		"the range is 2 s long, over the 1 s limit; shorten it to 1 s"
	);
	assert_eq!(violations[0]["suggestedDurationMs"], "1000");
	assert_eq!(violations[1]["suggestedWidth"], 100);
	assert!(
		violations[2]["message"]
			.as_str()
			.unwrap()
			.contains("is over the 0.0 MB limit"),
		"{}",
		violations[2]
	);

	// The same operation cannot be submitted.
	let (status, json) = post_json(
		&router,
		SUBMIT,
		serde_json::json!({ "videoToGif": operation }),
	)
	.await
	.unwrap();
	assert_ne!(status, StatusCode::OK);
	assert_eq!(json["code"], "failed_precondition", "{json}");
	let message = json["message"].as_str().unwrap();
	assert!(
		message.starts_with("the GIF would be over its limits: the range is 2 s long"),
		"{message}"
	);
	assert!(message.contains("reduce the width to 100 px"), "{message}");
	assert!(!root.path().join("big.gif").exists());
}

#[tokio::test]
async fn gif_requests_are_validated() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let out = |name: &str| root.path().join(name);
	let cases = [
		(
			serde_json::json!({}),
			ESTIMATE,
			"invalid_argument",
			"an operation is required",
		),
		(
			serde_json::json!({ "videoToGif": { "input": root.clip, "output": out("x.gif"), "startMs": 2000, "endMs": 2000 } }),
			SUBMIT,
			"invalid_argument",
			"the range must end after it starts",
		),
		(
			serde_json::json!({ "videoToGif": { "input": root.clip, "output": out("x.gif"), "startMs": 5000, "endMs": 6000 } }),
			SUBMIT,
			"invalid_argument",
			"starts after the end of the input",
		),
		(
			serde_json::json!({ "videoToGif": { "input": root.clip, "output": out("x.gif"), "endMs": 1000, "fps": 100 } }),
			SUBMIT,
			"invalid_argument",
			"the frame rate must be between 1 and 50",
		),
		(
			serde_json::json!({ "videoToGif": { "input": root.clip, "output": out("x.gif"), "startMs": -1, "endMs": 1000 } }),
			SUBMIT,
			"invalid_argument",
			"start_ms must not be negative",
		),
		(
			serde_json::json!({ "videoToGif": { "input": root.clip, "output": out("x.gif"), "endMs": 1000, "quality": 9 } }),
			SUBMIT,
			"invalid_argument",
			"unknown quality 9",
		),
		(
			serde_json::json!({ "videoToGif": { "input": root.audio, "output": out("x.gif"), "endMs": 1000 } }),
			SUBMIT,
			"invalid_argument",
			"the input has no video stream",
		),
		(
			serde_json::json!({ "videoToGif": { "input": root.clip, "output": root.clip, "endMs": 1000 } }),
			SUBMIT,
			"already_exists",
			"output file already exists",
		),
		(
			serde_json::json!({ "gifToVideo": { "input": root.clip, "output": out("x.mp4") } }),
			SUBMIT,
			"invalid_argument",
			"the input is not a GIF",
		),
		(
			serde_json::json!({ "gifToVideo": { "input": root.clip, "output": out("x.mp4"), "format": 7 } }),
			SUBMIT,
			"invalid_argument",
			"unknown video format 7",
		),
		(
			serde_json::json!({ "gifToVideo": { "input": "clip.gif", "output": out("x.mp4") } }),
			SUBMIT,
			"invalid_argument",
			"must be absolute",
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
	assert!(!out("x.gif").exists());
	assert!(!out("x.mp4").exists());
}
