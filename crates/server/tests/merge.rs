//! The merge end to end over the Connect router, as the dev frontend uses it
//! (JSON): the compatibility check, a merge job whose output lasts as long
//! as its parts, a merge blocked by a mismatch and unblocked by leaving the
//! stream out, and the request validation. Needs ffmpeg and ffprobe on
//! `PATH`.

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
const CHECK: &str = "/rpc/muxmultitool.merge.v1.MergeService/CheckMerge";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with three 320×240 H.264 + AAC clips of 2, 3 and 1 s, a
/// fourth whose audio is mono at 22.05 kHz, and a plain text file.
struct MediaRoot {
	dir: tempfile::TempDir,
	clips: Vec<PathBuf>,
	odd: PathBuf,
	text: PathBuf,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let runner = Runner::default();
		let mut clips = Vec::new();
		for (name, secs) in [("e01.mp4", 2), ("e02.mp4", 3), ("e03.mp4", 1)] {
			let path = dir.path().join(name);
			runner
				.run(&clip(&path, secs, "stereo", 48_000), &limits(), None)
				.await?;
			clips.push(path);
		}
		let odd = dir.path().join("odd.mp4");
		runner
			.run(&clip(&odd, 1, "mono", 22_050), &limits(), None)
			.await?;
		let text = dir.path().join("notes.txt");
		std::fs::write(&text, "not media")?;
		Ok(Self {
			dir,
			clips,
			odd,
			text,
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

	/// A merge of the first `n` clips into `output`.
	fn merge(&self, n: usize, output: &str) -> serde_json::Value {
		serde_json::json!({
			"inputs": self.clips[..n],
			"output": self.path().join(output),
		})
	}
}

fn limits() -> Limits {
	Limits::timeout(Duration::from_secs(60))
}

/// `secs` seconds of test pattern and tone, H.264 + AAC with the given
/// channel layout and sample rate.
fn clip(path: &Path, secs: u32, layout: &str, sample_rate: u32) -> FfmpegCommand {
	FfmpegCommand::new()
		.option("-f", "lavfi")
		.input(format!("testsrc2=duration={secs}:size=320x240:rate=25"))
		.option("-f", "lavfi")
		.input(format!("sine=frequency=440:duration={secs}"))
		.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
		.option("-ac", if layout == "mono" { "1" } else { "2" })
		.option("-ar", sample_rate.to_string())
		.output(path)
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

fn ms(value: &serde_json::Value) -> f64 {
	value
		.as_str()
		.and_then(|s| s.parse::<f64>().ok())
		.unwrap_or_default()
}

#[tokio::test]
async fn compatible_clips_are_checked_and_merged() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	// The check before running: the table, no problems, the command.
	let mut operation = root.merge(3, "e01-e03.mp4");
	operation["chapters"] = serde_json::json!(true);
	let (status, json) = post_json(
		&router,
		CHECK,
		serde_json::json!({ "operation": operation }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["canMerge"], true, "{json}");
	assert!(json.get("problems").is_none(), "{json}");
	let parts = json["parts"].as_array().unwrap();
	assert_eq!(parts.len(), 3);
	assert_eq!(parts[0]["name"], "e01.mp4");
	assert_eq!(parts[0]["layout"], "video, audio");
	assert!(
		(ms(&parts[1]["durationMs"]) - 3000.0).abs() < 200.0,
		"{json}"
	);
	let total = ms(&json["totalDurationMs"]);
	assert!((total - 6000.0).abs() < 300.0, "{json}");
	let streams = json["streams"].as_array().unwrap();
	assert_eq!(streams.len(), 2);
	assert_eq!(streams[0]["type"], "STREAM_TYPE_VIDEO");
	assert_eq!(
		streams[0]["cells"],
		serde_json::json!(["h264 320×240 25", "h264 320×240 25", "h264 320×240 25"])
	);
	assert_eq!(streams[0]["identical"], true);
	assert_eq!(streams[1]["type"], "STREAM_TYPE_AUDIO");
	assert_eq!(streams[1]["cells"][2], "aac stereo 48 kHz");
	let command = json["command"].as_str().unwrap();
	assert!(command.contains("-f concat -safe 0 -i"), "{command}");
	assert!(command.contains("-map_chapters 1 -c copy"), "{command}");
	assert!(command.contains("e01-e03.mp4"), "{command}");

	// The merge as a job.
	let job = run_job(&router, serde_json::json!({ "merge": operation }))
		.await
		.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(job["name"], "e01-e03.mp4");
	assert_eq!(job["command"], command, "the check showed the real command");
	assert_eq!(job["progress"]["percent"], 100.0, "{job}");

	// The output lasts as long as its parts and keeps their codecs.
	let output = root.path().join("e01-e03.mp4");
	let probe = Runner::default().probe(&output, &limits()).await.unwrap();
	let duration = probe.duration_secs().unwrap() * 1000.0;
	assert!(
		(duration - total).abs() < 200.0,
		"the output lasts {duration} ms, the parts {total} ms"
	);
	let video = probe.streams_of(CodecType::Video).next().unwrap();
	assert_eq!(video.codec_name.as_deref(), Some("h264"));
	assert_eq!(video.nb_frames, Some(150), "25 fps × 6 s");
	let audio = probe.streams_of(CodecType::Audio).next().unwrap();
	assert_eq!(audio.codec_name.as_deref(), Some("aac"));
	let titles: Vec<&str> = probe
		.chapters
		.iter()
		.filter_map(|c| c.tags.get("title").map(String::as_str))
		.collect();
	assert_eq!(titles, ["e01", "e02", "e03"]);
}

#[tokio::test]
async fn a_mismatch_blocks_the_merge_until_the_stream_is_left_out() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let mut operation = root.merge(2, "with-odd.mp4");
	operation["inputs"]
		.as_array_mut()
		.unwrap()
		.push(serde_json::json!(root.odd));

	let (status, json) = post_json(
		&router,
		CHECK,
		serde_json::json!({ "operation": operation }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_ne!(json["canMerge"], true, "{json}");
	let problems = json["problems"].as_array().unwrap();
	assert_eq!(problems.len(), 2, "{json}");
	assert_eq!(problems[0]["streamIndex"], 1);
	assert_eq!(problems[0]["part"], 2);
	assert_eq!(problems[0]["parameter"], "MERGE_PARAMETER_SAMPLE_RATE");
	assert_eq!(problems[0]["expected"], "48 kHz");
	assert_eq!(problems[0]["actual"], "22.05 kHz");
	assert_eq!(
		problems[0]["message"],
		"sample rate differs (odd.mp4): 22.05 kHz instead of 48 kHz"
	);
	assert_eq!(problems[1]["parameter"], "MERGE_PARAMETER_CHANNEL_LAYOUT");
	assert_eq!(
		problems[1]["message"],
		"channel layout differs (odd.mp4): mono instead of stereo"
	);
	let streams = json["streams"].as_array().unwrap();
	assert_eq!(streams[0]["identical"], true, "the video matches");
	assert!(streams[1].get("identical").is_none(), "{json}");
	assert_eq!(streams[1]["cells"][2], "aac mono 22.05 kHz");
	assert_eq!(streams[1]["mismatches"].as_array().unwrap().len(), 2);

	// The submit is refused with the same reason, and nothing is written.
	let (status, json) = post_json(&router, SUBMIT, serde_json::json!({ "merge": operation }))
		.await
		.unwrap();
	assert_ne!(status, StatusCode::OK);
	assert_eq!(json["code"], "failed_precondition", "{json}");
	assert_eq!(
		json["message"],
		"the files cannot be merged as they are: sample rate differs (odd.mp4): 22.05 kHz instead of 48 kHz; channel layout differs (odd.mp4): mono instead of stereo"
	);
	assert!(!root.path().join("with-odd.mp4").exists());

	// Leaving the audio out is the lossless fix.
	operation["dropStreams"] = serde_json::json!([1]);
	let (_, json) = post_json(
		&router,
		CHECK,
		serde_json::json!({ "operation": operation }),
	)
	.await
	.unwrap();
	assert_eq!(json["canMerge"], true, "{json}");
	assert_eq!(json["streams"][1]["dropped"], true, "{json}");
	assert!(
		json["command"]
			.as_str()
			.unwrap()
			.contains("-map 0 -map -0:1 -c copy"),
		"{json}"
	);
	let job = run_job(&router, serde_json::json!({ "merge": operation }))
		.await
		.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	let probe = Runner::default()
		.probe(&root.path().join("with-odd.mp4"), &limits())
		.await
		.unwrap();
	assert_eq!(probe.streams_of(CodecType::Audio).count(), 0);
	assert_eq!(
		probe.streams_of(CodecType::Video).next().unwrap().nb_frames,
		Some(150),
		"2 + 3 + 1 s of video"
	);
}

#[tokio::test]
async fn merge_requests_are_validated() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let out = |name: &str| root.path().join(name);
	let cases = [
		(
			serde_json::json!({}),
			CHECK,
			"invalid_argument",
			"an operation is required",
		),
		(
			serde_json::json!({ "operation": root.merge(1, "x.mp4") }),
			CHECK,
			"invalid_argument",
			"a merge needs at least 2 files",
		),
		(
			serde_json::json!({ "merge": root.merge(1, "x.mp4") }),
			SUBMIT,
			"invalid_argument",
			"a merge needs at least 2 files",
		),
		(
			serde_json::json!({ "merge": { "inputs": [root.clips[0], root.text], "output": out("x.mp4") } }),
			SUBMIT,
			"invalid_argument",
			"the input is not a media file",
		),
		(
			serde_json::json!({ "merge": { "inputs": [root.clips[0], "e02.mp4"], "output": out("x.mp4") } }),
			SUBMIT,
			"invalid_argument",
			"must be absolute",
		),
		(
			serde_json::json!({ "merge": { "inputs": [root.clips[0], out("missing.mp4")], "output": out("x.mp4") } }),
			SUBMIT,
			"not_found",
			"input file not found",
		),
		(
			serde_json::json!({ "merge": { "inputs": root.clips, "output": out("x.mp4"), "dropStreams": [5] } }),
			SUBMIT,
			"invalid_argument",
			"stream #5 is not a stream of the first file",
		),
		(
			serde_json::json!({ "merge": { "inputs": root.clips, "output": out("x.mp4"), "dropStreams": [0, 1] } }),
			SUBMIT,
			"invalid_argument",
			"nothing would be merged",
		),
		(
			serde_json::json!({ "merge": { "inputs": root.clips, "output": root.clips[0] } }),
			SUBMIT,
			"already_exists",
			"output file already exists",
		),
		(
			serde_json::json!({ "merge": { "inputs": root.clips, "output": "/elsewhere/x.mp4" } }),
			SUBMIT,
			"invalid_argument",
			"output folder does not exist",
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
}
