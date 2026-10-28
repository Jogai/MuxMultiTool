//! Extracting every track to its own file end to end over the Connect
//! router (JSON): the preview with its file names and skipped streams, a job
//! that writes the files (cover art as a real JPEG) and the refusals. Needs
//! ffmpeg and ffprobe on `PATH`.

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
const PREVIEW: &str = "/rpc/muxmultitool.extract.v1.ExtractService/PreviewExtract";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with `clip.mp4`: H.264 (0), AAC English (1), `mov_text`
/// subtitles (2) and a PNG cover (3).
struct MediaRoot {
	dir: tempfile::TempDir,
	clip: PathBuf,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(60));
		let subs = dir.path().join("subs.srt");
		std::fs::write(&subs, "1\n00:00:00,000 --> 00:00:01,000\nhello\n\n")?;
		let cover = dir.path().join("cover.png");
		let cmd = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("color=c=red:size=32x32")
			.option("-frames:v", "1")
			.output(&cover);
		runner.run(&cmd, &limits, None).await?;
		let clip = dir.path().join("clip.mp4");
		let cmd = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("testsrc2=duration=2:size=64x64:rate=10")
			.option("-f", "lavfi")
			.input("sine=frequency=440:duration=2")
			.input(&subs)
			.input(&cover)
			.args(["-map", "0", "-map", "1", "-map", "2", "-map", "3"])
			.option("-c:v:0", "libx264")
			.option("-preset", "ultrafast")
			.option("-pix_fmt:v:0", "yuv420p")
			.option("-c:v:1", "png")
			.option("-disposition:v:1", "attached_pic")
			.option("-c:a", "aac")
			.option("-c:s", "mov_text")
			.option("-metadata:s:a:0", "language=eng")
			.output(&clip);
		runner.run(&cmd, &limits, None).await?;
		std::fs::create_dir(dir.path().join("out"))?;
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

async fn post_json(router: &axum::Router, path: &str, body: Value) -> Result<(StatusCode, Value)> {
	let request = Request::post(path)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_string()))?;
	let response = router.clone().oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes();
	Ok((parts.status, serde_json::from_slice(&body)?))
}

async fn run_job(router: &axum::Router, operation: Value) -> Result<Value> {
	let (status, json) = post_json(router, SUBMIT, json!({ "extractStreams": operation })).await?;
	if status != StatusCode::OK {
		return Err(format!("submit refused: {json}").into());
	}
	let id = json["job"]["id"].as_str().unwrap_or_default().to_owned();
	tokio::time::timeout(Duration::from_secs(60), async {
		loop {
			let (_, json) = post_json(router, GET, json!({ "jobId": id })).await?;
			let state = json["job"]["state"].as_str().unwrap_or_default();
			if state != "JOB_STATE_QUEUED" && state != "JOB_STATE_RUNNING" {
				return Ok::<_, Box<dyn std::error::Error>>(json["job"].clone());
			}
			tokio::time::sleep(Duration::from_millis(50)).await;
		}
	})
	.await?
}

fn file_names(preview: &Value) -> Vec<String> {
	preview["files"]
		.as_array()
		.into_iter()
		.flatten()
		.map(|f| f["fileName"].as_str().unwrap_or_default().to_owned())
		.collect()
}

#[tokio::test]
async fn every_stream_is_written_to_its_own_file() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let operation = json!({ "input": root.clip, "outputFolder": root.path().join("out") });

	let (status, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{preview}");
	assert_eq!(preview["canRun"], true);
	assert_eq!(
		file_names(&preview),
		[
			"clip.s0.video.mkv",
			"clip.s1.audio.eng.aac",
			"clip.s2.subtitle.srt",
			"clip.s3.cover.jpg"
		]
	);

	let job = run_job(&router, operation).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(job["command"], preview["command"]);
	let out = |name: &str| root.path().join("out").join(name);
	assert_eq!(
		root.probe("out/clip.s0.video.mkv").await.unwrap(),
		[(CodecType::Video, "h264".into())]
	);
	assert_eq!(
		root.probe("out/clip.s1.audio.eng.aac").await.unwrap(),
		[(CodecType::Audio, "aac".into())]
	);
	assert_eq!(
		root.probe("out/clip.s2.subtitle.srt").await.unwrap(),
		[(CodecType::Subtitle, "subrip".into())]
	);
	// The PNG cover is a JPEG now, by content and not only by name.
	assert_eq!(
		root.probe("out/clip.s3.cover.jpg").await.unwrap(),
		[(CodecType::Video, "mjpeg".into())]
	);
	let bytes = std::fs::read(out("clip.s3.cover.jpg")).unwrap();
	assert_eq!(bytes[..3], [0xFF, 0xD8, 0xFF]);
}

#[tokio::test]
async fn a_subset_next_to_the_input_and_the_refusals() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	let operation = json!({
		"input": root.clip,
		"mode": "SELECTION_MODE_KEEP",
		"streamIndexes": [3]
	});
	let job = run_job(&router, operation.clone()).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert!(root.path().join("clip.s3.cover.jpg").is_file());
	assert!(!root.path().join("clip.s0.video.mkv").exists());

	// The file exists now: a second submit is refused and nothing changes.
	let (status, body) = post_json(&router, SUBMIT, json!({ "extractStreams": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::CONFLICT, "{body}");

	// A stream without a file format that was ticked by hand blocks; there
	// is none in this clip, so an unknown stream is an invalid argument.
	let (status, body) = post_json(
		&router,
		PREVIEW,
		json!({ "operation": {
			"input": root.clip, "mode": "SELECTION_MODE_KEEP", "streamIndexes": [9]
		}}),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

	// A folder outside the media root is refused.
	let (status, body) = post_json(
		&router,
		PREVIEW,
		json!({ "operation": { "input": root.clip, "outputFolder": "/" }}),
	)
	.await
	.unwrap();
	assert_ne!(status, StatusCode::OK, "{body}");
}
