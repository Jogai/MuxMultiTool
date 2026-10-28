//! The split service over the Connect router: parts by duration, size and
//! count snapped to keyframes, and the returned operation going through
//! `PlanCut`. Needs ffmpeg and ffprobe on `PATH`.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner};
use muxmultitool_server::{Config, app};
use tower::ServiceExt;

const SPLIT: &str = "/rpc/muxmultitool.cut.v1.SplitService/PlanSplit";
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

fn ranges(json: &serde_json::Value) -> Vec<(f64, f64)> {
	json["parts"]
		.as_array()
		.into_iter()
		.flatten()
		.map(|p| {
			(
				p["startSeconds"].as_f64().unwrap_or(0.0),
				p["endSeconds"].as_f64().unwrap_or(f64::NAN),
			)
		})
		.collect()
}

#[tokio::test]
async fn splits_by_duration_on_keyframes_and_plans_the_cut() -> Result<()> {
	let media = MediaRoot::new().await?;
	let router = app(&media.config());
	let output = media.path().join("part.mp4");
	let (status, json) = post_json(
		&router,
		SPLIT,
		serde_json::json!({ "fileId": media.clip, "maxDurationSeconds": 2.5, "output": output }),
	)
	.await?;
	assert_eq!(status, StatusCode::OK, "{json}");
	let parts = ranges(&json);
	assert_eq!(parts.len(), 3, "{json}");
	for (part, (start, end)) in parts.iter().zip([(0.0, 2.0), (2.0, 4.0), (4.0, 6.0)]) {
		assert!(
			(part.0 - start).abs() < 0.1 && (part.1 - end).abs() < 0.15,
			"{parts:?}"
		);
	}
	let (status, plan) = post_json(
		&router,
		PLAN,
		serde_json::json!({ "operation": json["operation"] }),
	)
	.await?;
	assert_eq!(status, StatusCode::OK, "{plan}");
	assert_eq!(plan["outputs"].as_array().map_or(0, Vec::len), 3);
	Ok(())
}

#[tokio::test]
async fn splits_by_size_and_count() -> Result<()> {
	let media = MediaRoot::new().await?;
	let router = app(&media.config());
	let size = std::fs::metadata(&media.clip)?.len();
	let limit = size / 2;
	let (status, json) = post_json(
		&router,
		SPLIT,
		serde_json::json!({ "fileId": media.clip, "maxSizeBytes": limit }),
	)
	.await?;
	assert_eq!(status, StatusCode::OK, "{json}");
	assert!(ranges(&json).len() >= 3, "{json}");
	for part in json["parts"].as_array().into_iter().flatten() {
		let bytes: u64 = part["estimatedBytes"].as_str().unwrap_or("0").parse()?;
		assert!(bytes <= limit, "{part}");
	}
	let (status, json) = post_json(
		&router,
		SPLIT,
		serde_json::json!({ "fileId": media.clip, "parts": 2 }),
	)
	.await?;
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(ranges(&json).len(), 2);
	Ok(())
}

#[tokio::test]
async fn refuses_a_missing_mode_and_a_bad_limit() -> Result<()> {
	let media = MediaRoot::new().await?;
	let router = app(&media.config());
	let (status, _) =
		post_json(&router, SPLIT, serde_json::json!({ "fileId": media.clip })).await?;
	assert_eq!(status, StatusCode::BAD_REQUEST);
	let (status, _) = post_json(
		&router,
		SPLIT,
		serde_json::json!({ "fileId": media.clip, "parts": 0 }),
	)
	.await?;
	assert_eq!(status, StatusCode::BAD_REQUEST);
	Ok(())
}
