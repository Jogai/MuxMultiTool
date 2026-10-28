//! Snapshots and cover art end to end: the frame route, the snapshot job and
//! the cover-art job over the in-process router, with real ffmpeg and
//! ffprobe from `PATH`.

#![expect(
	clippy::unwrap_used,
	reason = "a failed unwrap fails the test, and the helpers are shared by tests"
)]

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, ProbeResult, Runner};
use muxmultitool_server::{Config, app};
use tower::ServiceExt;

const SUBMIT: &str = "/rpc/muxmultitool.jobs.v1.JobsService/SubmitJob";
const GET: &str = "/rpc/muxmultitool.jobs.v1.JobsService/GetJob";
const START: &str = "/rpc/muxmultitool.media.v1.MediaService/StartPreview";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with a 640×360 MPEG-4 clip in MKV (the preview has to
/// transcode it, so a frame taken from the preview would not match the
/// source's codec path) and an H.264 + AAC clip in MP4.
struct MediaRoot {
	dir: tempfile::TempDir,
	mpeg4: PathBuf,
	h264: PathBuf,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(60));
		let make = |video: &[&str], output: &Path| {
			let mut cmd = FfmpegCommand::new()
				.option("-f", "lavfi")
				.input("testsrc2=duration=4:size=640x360:rate=25")
				.option("-f", "lavfi")
				.input("sine=frequency=440:duration=4");
			for pair in video.chunks(2) {
				cmd = cmd.option(pair[0], pair[1]);
			}
			cmd.option("-pix_fmt", "yuv420p")
				.option("-c:a", "aac")
				.output(output)
		};
		let mpeg4 = dir.path().join("old.mkv");
		runner
			.run(&make(&["-c:v", "mpeg4"], &mpeg4), &limits, None)
			.await?;
		let h264 = dir.path().join("new.mp4");
		runner
			.run(
				&make(&["-c:v", "libx264", "-preset", "ultrafast"], &h264),
				&limits,
				None,
			)
			.await?;
		Ok(Self { dir, mpeg4, h264 })
	}

	fn path(&self) -> &Path {
		self.dir.path()
	}

	fn router(&self) -> Router {
		app(&Config {
			address: IpAddr::V4(Ipv4Addr::LOCALHOST),
			port: 0,
			media_roots: vec![self.path().to_path_buf()],
			frontend_dir: self.path().join("no-frontend"),
			..Config::default()
		})
	}

	fn out(&self, name: &str) -> PathBuf {
		self.path().join(name)
	}
}

async fn send(router: &Router, request: Request<Body>) -> Result<(http::response::Parts, Vec<u8>)> {
	let response = router.clone().oneshot(request).await?;
	let (parts, body) = response.into_parts();
	Ok((parts, body.collect().await?.to_bytes().to_vec()))
}

async fn post_json(
	router: &Router,
	path: &str,
	body: serde_json::Value,
) -> Result<(StatusCode, serde_json::Value)> {
	let request = Request::post(path)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_string()))?;
	let (parts, body) = send(router, request).await?;
	Ok((parts.status, serde_json::from_slice(&body)?))
}

async fn get(router: &Router, uri: &str) -> Result<(http::response::Parts, Vec<u8>)> {
	send(router, Request::get(uri).body(Body::empty())?).await
}

/// Submits an operation and waits for the job; the error is the refusal body.
async fn run_job(
	router: &Router,
	operation: serde_json::Value,
) -> std::result::Result<serde_json::Value, serde_json::Value> {
	let (status, json) = post_json(router, SUBMIT, operation).await.unwrap();
	if status != StatusCode::OK {
		return Err(json);
	}
	let id = json["job"]["id"].as_str().unwrap().to_owned();
	tokio::time::timeout(Duration::from_secs(60), async {
		loop {
			let (_, json) = post_json(router, GET, serde_json::json!({ "jobId": id }))
				.await
				.unwrap();
			let state = json["job"]["state"].as_str().unwrap_or_default();
			if state != "JOB_STATE_QUEUED" && state != "JOB_STATE_RUNNING" {
				return Ok(json["job"].clone());
			}
			tokio::time::sleep(Duration::from_millis(50)).await;
		}
	})
	.await
	.unwrap()
}

async fn probe(path: &Path) -> ProbeResult {
	Runner::default()
		.probe(path, &Limits::timeout(Duration::from_secs(30)))
		.await
		.unwrap()
}

/// The size of the picture in `bytes`, via ffprobe on a copy.
async fn picture_size(root: &MediaRoot, name: &str, bytes: &[u8]) -> (u32, u32) {
	let path = root.out(name);
	std::fs::write(&path, bytes).unwrap();
	let probe = probe(&path).await;
	let video = probe.streams_of(CodecType::Video).next().unwrap();
	(video.width.unwrap(), video.height.unwrap())
}

#[tokio::test]
async fn the_frame_route_serves_the_original_at_full_resolution() {
	let root = MediaRoot::new().await.unwrap();
	let router = root.router();
	let (status, json) = post_json(&router, START, serde_json::json!({ "fileId": root.mpeg4 }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	let media_id = json["mediaId"].as_str().unwrap();

	let (parts, jpeg) = get(&router, &format!("/media/{media_id}/frame?t=1.5"))
		.await
		.unwrap();
	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(parts.headers[header::CONTENT_TYPE], "image/jpeg");
	assert_eq!(&jpeg[..3], [0xFF, 0xD8, 0xFF], "a JPEG");
	assert_eq!(picture_size(&root, "got.jpg", &jpeg).await, (640, 360));

	let (parts, png) = get(&router, &format!("/media/{media_id}/frame?t=2&format=png"))
		.await
		.unwrap();
	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(parts.headers[header::CONTENT_TYPE], "image/png");
	assert_eq!(&png[..4], b"\x89PNG");
	assert_eq!(picture_size(&root, "got.png", &png).await, (640, 360));

	// Different positions give different pictures
	let (_, later) = get(&router, &format!("/media/{media_id}/frame?t=3.5"))
		.await
		.unwrap();
	assert_ne!(jpeg, later);

	// The query is percent-decoded: the same frame as `t=1.5&format=jpg`
	let (parts, decoded) = get(
		&router,
		&format!("/media/{media_id}/frame?t=1%2E5&format=%6Apg"),
	)
	.await
	.unwrap();
	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(decoded, jpeg);

	// A file id works without a preview session
	let file_id = root.h264.display().to_string().replace('/', "%2F");
	let (parts, _) = get(&router, &format!("/media/{file_id}/frame?t=0"))
		.await
		.unwrap();
	assert_eq!(parts.status, StatusCode::OK);
}

#[tokio::test]
async fn the_frame_route_refuses_bad_requests() {
	let root = MediaRoot::new().await.unwrap();
	let router = root.router();
	let file_id = root.h264.display().to_string().replace('/', "%2F");
	for (uri, expected) in [
		(format!("/media/{file_id}/frame"), StatusCode::BAD_REQUEST),
		(
			format!("/media/{file_id}/frame?t=-1"),
			StatusCode::BAD_REQUEST,
		),
		(
			format!("/media/{file_id}/frame?t=x"),
			StatusCode::BAD_REQUEST,
		),
		(
			format!("/media/{file_id}/frame?t=1&format=gif"),
			StatusCode::BAD_REQUEST,
		),
		(
			format!("/media/{file_id}/frame?t=100"),
			StatusCode::UNPROCESSABLE_ENTITY,
		),
		(
			"/media/nosuchid/frame?t=1".to_owned(),
			StatusCode::BAD_REQUEST,
		),
	] {
		let (parts, body) = get(&router, &uri).await.unwrap();
		assert_eq!(
			parts.status,
			expected,
			"{uri}: {}",
			String::from_utf8_lossy(&body)
		);
	}
	// Outside the roots
	let (parts, _) = get(&router, "/media/%2Fetc%2Fhostname/frame?t=0")
		.await
		.unwrap();
	assert!(
		matches!(parts.status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
		"{}",
		parts.status
	);
}

#[tokio::test]
async fn a_snapshot_job_saves_png_and_jpeg_files() {
	let root = MediaRoot::new().await.unwrap();
	let router = root.router();
	for (name, magic) in [
		("shot.png", &b"\x89PNG"[..]),
		("shot.JPG", &[0xFF, 0xD8][..]),
	] {
		let output = root.out(name);
		let job = run_job(
			&router,
			serde_json::json!({ "snapshot": {
				"input": root.mpeg4, "output": output, "positionMs": 1500,
			}}),
		)
		.await
		.unwrap();
		assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
		let bytes = std::fs::read(&output).unwrap();
		assert!(bytes.starts_with(magic), "{name}");
		assert_eq!(picture_size(&root, "check.bin", &bytes).await.0, 640);
	}
}

#[tokio::test]
async fn a_snapshot_is_refused_for_a_bad_output_or_position() {
	let root = MediaRoot::new().await.unwrap();
	let router = root.router();
	let refused = |output: &str, position: i64| {
		serde_json::json!({ "snapshot": {
			"input": root.h264, "output": root.out(output), "positionMs": position,
		}})
	};
	let error = run_job(&router, refused("shot.gif", 0)).await.unwrap_err();
	assert_eq!(error["code"], "invalid_argument", "{error}");
	assert!(error["message"].as_str().unwrap().contains(".png"));
	let error = run_job(&router, refused("shot.png", -1)).await.unwrap_err();
	assert_eq!(error["code"], "invalid_argument", "{error}");
	let error = run_job(&router, refused("shot.png", 60_000))
		.await
		.unwrap_err();
	assert_eq!(error["code"], "invalid_argument", "{error}");
	assert!(!root.out("shot.png").exists());
}

fn cover_streams(probe: &ProbeResult) -> usize {
	probe
		.streams
		.iter()
		.filter(|s| s.disposition.attached_pic)
		.count()
}

#[tokio::test]
async fn cover_art_is_embedded_in_mp4_and_mkv_without_re_encoding_the_rest() {
	let root = MediaRoot::new().await.unwrap();
	let router = root.router();
	let before = probe(&root.h264).await;
	assert_eq!(cover_streams(&before), 0);

	for (input, name) in [
		(&root.h264, "with-cover.mp4"),
		(&root.h264, "with-cover.mkv"),
	] {
		let output = root.out(name);
		let job = run_job(
			&router,
			serde_json::json!({ "coverArt": {
				"input": input, "output": output, "positionMs": 2000,
			}}),
		)
		.await
		.unwrap();
		assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{name}: {job}");
		let after = probe(&output).await;
		assert_eq!(cover_streams(&after), 1, "{name}");
		let cover = after
			.streams
			.iter()
			.find(|s| s.disposition.attached_pic)
			.unwrap();
		assert_eq!(cover.codec_name.as_deref(), Some("mjpeg"), "{name}");
		assert_eq!(cover.width, Some(640), "full resolution, {name}");
		// The other streams are the originals, copied
		let original: Vec<_> = before
			.streams
			.iter()
			.map(|s| (s.codec_type, s.codec_name.clone(), s.width))
			.collect();
		let kept: Vec<_> = after
			.streams
			.iter()
			.filter(|s| !s.disposition.attached_pic)
			.map(|s| (s.codec_type, s.codec_name.clone(), s.width))
			.collect();
		assert_eq!(kept, original, "{name}");
	}

	// Setting it again replaces the cover instead of adding a second one
	let again = root.out("again.mkv");
	let job = run_job(
		&router,
		serde_json::json!({ "coverArt": {
			"input": root.out("with-cover.mkv"), "output": again, "positionMs": 500,
		}}),
	)
	.await
	.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(cover_streams(&probe(&again).await), 1);

	let again = root.out("again.mp4");
	let job = run_job(
		&router,
		serde_json::json!({ "coverArt": {
			"input": root.out("with-cover.mp4"), "output": again, "positionMs": 500,
		}}),
	)
	.await
	.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(cover_streams(&probe(&again).await), 1);
}

#[tokio::test]
async fn cover_art_is_refused_for_other_containers_with_the_reason() {
	let root = MediaRoot::new().await.unwrap();
	let router = root.router();
	for name in ["cover.webm", "cover.mov", "cover.avi"] {
		let error = run_job(
			&router,
			serde_json::json!({ "coverArt": {
				"input": root.h264, "output": root.out(name), "positionMs": 0,
			}}),
		)
		.await
		.unwrap_err();
		assert_eq!(error["code"], "failed_precondition", "{name}: {error}");
		assert!(
			error["message"]
				.as_str()
				.unwrap()
				.contains("only be embedded in MKV and MP4"),
			"{error}"
		);
		assert!(!root.out(name).exists());
	}
}

#[tokio::test]
async fn cover_art_picture_files_are_removed_when_the_job_ends() {
	let root = MediaRoot::new().await.unwrap();
	let router = root.router();
	let job = run_job(
		&router,
		serde_json::json!({ "coverArt": {
			"input": root.h264, "output": root.out("c.mkv"), "positionMs": 1000,
		}}),
	)
	.await
	.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	// The job's command names the picture; give the cleanup task a moment
	let command = job["command"].as_str().unwrap();
	let picture = command
		.split_whitespace()
		.find(|part| part.contains("muxmultitool-frame-"))
		.unwrap()
		.trim_matches('\'');
	tokio::time::timeout(Duration::from_secs(5), async {
		while Path::new(picture).exists() {
			tokio::time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("the temporary picture is removed");
}
