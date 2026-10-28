//! The timeline RPCs and the sprite route end to end: the Connect streaming
//! envelope a browser reads, opaque and path file ids, and the errors.
//! Runs the real ffmpeg and ffprobe from `PATH`.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner};
use muxmultitool_core::files::Location;
use muxmultitool_server::{Config, app};
use tower::ServiceExt;

const THUMBNAILS: &str = "/rpc/muxmultitool.media.v1.MediaService/GetThumbnails";
const WAVEFORM: &str = "/rpc/muxmultitool.media.v1.MediaService/GetWaveform";
const CUES: &str = "/rpc/muxmultitool.media.v1.MediaService/GetSubtitleCues";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Four seconds of test pattern, a tone for the first two seconds, and two
/// SubRip cues, in `dir/clip.mkv`: streams 0, 1 and 2.
async fn make_clip(dir: &Path) -> Result<PathBuf> {
	let subs = dir.join("subs.vtt");
	std::fs::write(
		&subs,
		"WEBVTT\n\n00:00.500 --> 00:01.500\nHello <i>there</i>\n\n00:02.000 --> 00:03.000\nSecond\nline two\n",
	)?;
	let clip = dir.join("clip.mkv");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=size=160x120:rate=25:duration=4")
		.option("-f", "lavfi")
		.input("aevalsrc=if(lt(t\\,2)\\,0.5*sin(440*2*PI*t)\\,0):s=48000:d=4")
		.input(&subs)
		.option("-map", "0:v")
		.option("-map", "1:a")
		.option("-map", "2:s")
		.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
		.option("-c:s", "srt")
		.flag("-y")
		.output(&clip);
	Runner::default()
		.run(&cmd, &Limits::timeout(Duration::from_secs(120)), None)
		.await?;
	Ok(clip)
}

fn router(media_root: &Path) -> Router {
	app(&Config {
		address: IpAddr::V4(Ipv4Addr::LOCALHOST),
		port: 0,
		media_roots: vec![media_root.to_path_buf()],
		..Config::default()
	})
}

async fn send(router: &Router, request: Request<Body>) -> Result<(http::response::Parts, Vec<u8>)> {
	let response = router.clone().oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes().to_vec();
	Ok((parts, body))
}

/// Calls a server-streaming method with the Connect envelope and returns
/// the frames: `(flags, payload)`, the last one being the end-stream frame.
async fn stream(
	router: &Router,
	path: &str,
	json: serde_json::Value,
) -> Result<Vec<(u8, serde_json::Value)>> {
	let request_body = json.to_string();
	let mut body = vec![0u8];
	body.extend_from_slice(&u32::try_from(request_body.len())?.to_be_bytes());
	body.extend_from_slice(request_body.as_bytes());
	let request = Request::post(path)
		.header(header::CONTENT_TYPE, "application/connect+json")
		.header("connect-protocol-version", "1")
		.body(Body::from(body))?;
	let (parts, body) = send(router, request).await?;
	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(
		parts.headers[header::CONTENT_TYPE],
		"application/connect+json"
	);
	let mut frames = Vec::new();
	let mut rest = body.as_slice();
	while rest.len() >= 5 {
		let flags = rest[0];
		let len = u32::from_be_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
		let payload = &rest[5..5 + len];
		frames.push((flags, serde_json::from_slice(payload).unwrap_or_default()));
		rest = &rest[5 + len..];
	}
	assert!(
		frames.last().is_some_and(|(flags, _)| *flags & 0b10 != 0),
		"ends with an end-stream frame: {frames:?}"
	);
	Ok(frames)
}

/// The data frames and the error code of the end-stream frame, if any.
fn split(frames: &[(u8, serde_json::Value)]) -> (Vec<&serde_json::Value>, Option<String>) {
	let Some((end, data)) = frames.split_last() else {
		return (Vec::new(), None);
	};
	let code = end.1["error"]["code"].as_str().map(str::to_owned);
	(data.iter().map(|(_, json)| json).collect(), code)
}

async fn get(router: &Router, path: &str) -> Result<(http::response::Parts, Vec<u8>)> {
	send(router, Request::get(path).body(Body::empty())?).await
}

#[tokio::test]
async fn thumbnails_are_streamed_and_served_as_sprites() {
	let dir = tempfile::tempdir().unwrap();
	let clip = make_clip(dir.path()).await.unwrap();
	let router = router(dir.path());

	let frames = stream(&router, THUMBNAILS, serde_json::json!({ "fileId": clip }))
		.await
		.unwrap();
	let (data, error) = split(&frames);
	assert_eq!(error, None, "{frames:?}");
	let last = data.last().unwrap();
	assert_eq!(last["done"], true, "{frames:?}");
	let sheet = &last["sheet"];
	assert_eq!(sheet["tileWidth"], 160);
	assert_eq!(sheet["tileHeight"], 120);
	assert_eq!(sheet["columns"], 10);
	assert_eq!(sheet["rows"], 10);
	assert_eq!(sheet["intervalSeconds"], 0.5);
	assert_eq!(sheet["spriteCount"], 1, "{sheet}");
	assert_eq!(last["spritesReady"], 1, "{last}");
	let id = sheet["mediaId"].as_str().unwrap();
	assert_eq!(
		sheet["spriteUrlTemplate"],
		format!("/media/{id}/thumbs/{{n}}.jpg")
	);
	assert!(
		data.iter().all(|f| f["sheet"]["mediaId"] == id),
		"every message carries the sheet: {data:?}"
	);

	let (parts, body) = get(&router, &format!("/media/{id}/thumbs/0.jpg"))
		.await
		.unwrap();
	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(parts.headers[header::CONTENT_TYPE], "image/jpeg");
	assert!(
		parts.headers[header::CACHE_CONTROL]
			.to_str()
			.unwrap()
			.contains("max-age")
	);
	assert_eq!(&body[..2], &[0xff, 0xd8], "a JPEG");
	let request = Request::get(format!("/media/{id}/thumbs/0.jpg"))
		.header(header::RANGE, "bytes=0-1")
		.body(Body::empty())
		.unwrap();
	let (parts, head) = send(&router, request).await.unwrap();
	assert_eq!(parts.status, StatusCode::PARTIAL_CONTENT);
	assert_eq!(head, [0xff, 0xd8]);
	assert!(parts.headers.contains_key(header::LAST_MODIFIED));
	for path in [
		format!("/media/{id}/thumbs/1.jpg"),
		format!("/media/{id}/thumbs/x.jpg"),
		format!("/media/{id}/thumbs/0.png"),
		String::from("/media/nope/thumbs/0.jpg"),
	] {
		let (parts, _) = get(&router, &path).await.unwrap();
		assert_eq!(parts.status, StatusCode::NOT_FOUND, "{path}");
	}

	// The same file by its opaque id: the same cached result, in one message.
	let opaque = Location::InRoot {
		root: 0,
		relative: PathBuf::from("clip.mkv"),
	}
	.id();
	let frames = stream(
		&router,
		THUMBNAILS,
		serde_json::json!({ "fileId": opaque.as_str() }),
	)
	.await
	.unwrap();
	let (data, error) = split(&frames);
	assert_eq!(error, None);
	assert_eq!(data.len(), 1, "{data:?}");
	assert_eq!(data[0]["sheet"]["mediaId"], id);
	assert_eq!(data[0]["done"], true);
}

#[tokio::test]
async fn waveform_peaks_arrive_in_order_for_the_requested_window() {
	let dir = tempfile::tempdir().unwrap();
	let clip = make_clip(dir.path()).await.unwrap();
	let router = router(dir.path());

	let frames = stream(
		&router,
		WAVEFORM,
		serde_json::json!({ "fileId": clip, "streamIndex": 1, "peakCount": 8 }),
	)
	.await
	.unwrap();
	let (data, error) = split(&frames);
	assert_eq!(error, None, "{frames:?}");
	let mut mins = Vec::new();
	let mut maxs = Vec::new();
	for message in &data {
		// Proto JSON leaves default values out: a missing field is 0.
		assert_eq!(message["peakCount"], 8);
		assert_eq!(message["fromSeconds"].as_f64().unwrap_or(0.0), 0.0);
		assert_eq!(
			usize::try_from(message["offset"].as_u64().unwrap_or(0)).unwrap(),
			mins.len(),
			"{message}"
		);
		let values = |key: &str| -> Vec<f64> {
			message[key]
				.as_array()
				.into_iter()
				.flatten()
				.map(|v| v.as_f64().unwrap())
				.collect()
		};
		mins.extend(values("mins"));
		maxs.extend(values("maxs"));
	}
	let last = data.last().unwrap();
	assert_eq!(last["done"], true, "{last}");
	assert!(last["toSeconds"].as_f64().unwrap() > 3.9, "{last}");
	assert!(last["analyzedSeconds"].as_f64().unwrap() > 3.9, "{last}");
	assert_eq!((mins.len(), maxs.len()), (8, 8));
	assert!(
		maxs[..4].iter().all(|m| (0.4..=0.6).contains(m)),
		"{maxs:?}"
	);
	assert!(
		mins[..4].iter().all(|m| (-0.6..=-0.4).contains(m)),
		"{mins:?}"
	);
	assert!(maxs[4..].iter().all(|m| m.abs() < 0.05), "{maxs:?}");

	// Errors: a bad window, the video stream, a missing stream.
	for (request, code) in [
		(
			serde_json::json!({ "fileId": clip, "streamIndex": 1, "peakCount": 0 }),
			"invalid_argument",
		),
		(
			serde_json::json!({ "fileId": clip, "streamIndex": 1, "fromSeconds": 9, "peakCount": 10 }),
			"invalid_argument",
		),
		(
			serde_json::json!({ "fileId": clip, "streamIndex": 0, "peakCount": 10 }),
			"failed_precondition",
		),
		(
			serde_json::json!({ "fileId": clip, "streamIndex": 7, "peakCount": 10 }),
			"not_found",
		),
		(
			serde_json::json!({ "fileId": dir.path().join("missing.mkv"), "streamIndex": 1, "peakCount": 10 }),
			"not_found",
		),
	] {
		let frames = stream(&router, WAVEFORM, request.clone()).await.unwrap();
		let (data, error) = split(&frames);
		assert_eq!(data.len(), 0, "{request}: {frames:?}");
		assert_eq!(error.as_deref(), Some(code), "{request}: {frames:?}");
	}
}

#[tokio::test]
async fn subtitle_cues_are_streamed_and_other_streams_refused() {
	let dir = tempfile::tempdir().unwrap();
	let clip = make_clip(dir.path()).await.unwrap();
	let router = router(dir.path());

	let frames = stream(
		&router,
		CUES,
		serde_json::json!({ "fileId": clip, "streamIndex": 2 }),
	)
	.await
	.unwrap();
	let (data, error) = split(&frames);
	assert_eq!(error, None, "{frames:?}");
	assert_eq!(data.last().unwrap()["done"], true);
	let cues: Vec<&serde_json::Value> = data
		.iter()
		.flat_map(|m| m["cues"].as_array().into_iter().flatten())
		.collect();
	assert_eq!(cues.len(), 2, "{data:?}");
	assert_eq!(cues[0]["startSeconds"], 0.5);
	assert_eq!(cues[0]["endSeconds"], 1.5);
	assert_eq!(cues[0]["text"], "Hello there");
	assert_eq!(cues[1]["text"], "Second\nline two");

	let frames = stream(
		&router,
		CUES,
		serde_json::json!({ "fileId": clip, "streamIndex": 1 }),
	)
	.await
	.unwrap();
	let (data, error) = split(&frames);
	assert_eq!(data.len(), 0, "{frames:?}");
	assert_eq!(error.as_deref(), Some("failed_precondition"), "{frames:?}");

	// Outside the media roots, and a relative path.
	let other = tempfile::tempdir().unwrap();
	let outside = make_clip(other.path()).await.unwrap();
	let frames = stream(
		&router,
		CUES,
		serde_json::json!({ "fileId": outside, "streamIndex": 2 }),
	)
	.await
	.unwrap();
	assert_eq!(split(&frames).1.as_deref(), Some("permission_denied"));
	let frames = stream(
		&router,
		CUES,
		serde_json::json!({ "fileId": "clip.mkv", "streamIndex": 2 }),
	)
	.await
	.unwrap();
	assert_eq!(split(&frames).1.as_deref(), Some("invalid_argument"));
}
