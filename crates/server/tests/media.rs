//! The media service and the HLS routes, from the RPC to the segment bytes.
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

const START: &str = "/rpc/muxmultitool.media.v1.MediaService/StartPreview";
const STOP: &str = "/rpc/muxmultitool.media.v1.MediaService/StopPreview";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Four seconds of H.264 + AAC in an MKV, in `dir`.
async fn make_clip(dir: &Path) -> Result<std::path::PathBuf> {
	let clip = dir.join("clip.mkv");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=size=160x120:rate=25:duration=4")
		.option("-f", "lavfi")
		.input("sine=frequency=440:sample_rate=48000:duration=4")
		.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-pix_fmt", "yuv420p")
		.option("-g", "50")
		.option("-c:a", "aac")
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

/// The id `FilesService` issues for `name` in the first media root.
fn id_of(name: &str) -> String {
	Location::InRoot {
		root: 0,
		relative: PathBuf::from(name),
	}
	.id()
	.as_str()
	.to_owned()
}

/// The id `OpenPaths` issues for a native-dialog path in desktop mode.
fn absolute_id(path: &Path) -> String {
	Location::Absolute(path.to_path_buf())
		.id()
		.as_str()
		.to_owned()
}

async fn send(router: &Router, request: Request<Body>) -> Result<(http::response::Parts, Vec<u8>)> {
	let response = router.clone().oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes().to_vec();
	Ok((parts, body))
}

async fn rpc(
	router: &Router,
	path: &str,
	json: serde_json::Value,
) -> Result<(StatusCode, serde_json::Value)> {
	let request = Request::post(path)
		.header(header::HOST, "127.0.0.1")
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(json.to_string()))?;
	let (parts, body) = send(router, request).await?;
	Ok((parts.status, serde_json::from_slice(&body)?))
}

async fn get(router: &Router, path: &str) -> Result<(http::response::Parts, Vec<u8>)> {
	send(router, Request::get(path).body(Body::empty())?).await
}

#[tokio::test]
async fn start_preview_then_fetch_playlist_and_segments() {
	let dir = tempfile::tempdir().unwrap();
	make_clip(dir.path()).await.unwrap();
	let clip = id_of("clip.mkv");
	let router = router(dir.path());

	let (status, json) = rpc(&router, START, serde_json::json!({ "fileId": clip }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	assert_eq!(json["mode"], "PREVIEW_MODE_REMUX", "{json}");
	assert_eq!(json["segmentCount"], 2, "{json}");
	let id = json["mediaId"].as_str().unwrap();
	let playlist_url = json["playlistUrl"].as_str().unwrap();
	assert_eq!(playlist_url, format!("/media/{id}/hls/index.m3u8"));

	let (parts, body) = get(&router, playlist_url).await.unwrap();
	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(
		parts.headers[header::CONTENT_TYPE],
		"application/vnd.apple.mpegurl"
	);
	let playlist = String::from_utf8(body).unwrap();
	assert!(playlist.starts_with("#EXTM3U\n"), "{playlist}");
	assert!(playlist.contains("\n1.ts\n#EXT-X-ENDLIST\n"), "{playlist}");

	for n in [1, 0] {
		let (parts, body) = get(&router, &format!("/media/{id}/hls/{n}.ts"))
			.await
			.unwrap();
		assert_eq!(parts.status, StatusCode::OK, "segment {n}");
		assert_eq!(parts.headers[header::CONTENT_TYPE], "video/mp2t");
		assert_eq!(body[0], 0x47, "an MPEG-TS sync byte");
		assert!(body.len() > 188 * 10, "segment {n} is {} bytes", body.len());
	}

	// Range requests, as a player's media source may send them
	let request = Request::get(format!("/media/{id}/hls/0.ts"))
		.header(header::RANGE, "bytes=188-375")
		.body(Body::empty())
		.unwrap();
	let (parts, body) = send(&router, request).await.unwrap();
	assert_eq!(parts.status, StatusCode::PARTIAL_CONTENT);
	assert_eq!(body.len(), 188);
	assert_eq!(body[0], 0x47, "the second packet starts with a sync byte");
	assert!(parts.headers.contains_key(header::LAST_MODIFIED));
	assert_eq!(
		parts.headers[header::CACHE_CONTROL],
		"private, max-age=3600"
	);

	// Same file: same session.
	let (_, again) = rpc(&router, START, serde_json::json!({ "fileId": clip }))
		.await
		.unwrap();
	assert_eq!(again["mediaId"], id);

	let (parts, _) = get(&router, &format!("/media/{id}/hls/9.ts"))
		.await
		.unwrap();
	assert_eq!(parts.status, StatusCode::NOT_FOUND);
	let (parts, _) = get(&router, &format!("/media/{id}/hls/x.ts"))
		.await
		.unwrap();
	assert_eq!(parts.status, StatusCode::NOT_FOUND);

	let (status, _) = rpc(&router, STOP, serde_json::json!({ "mediaId": id }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK);
	let (parts, _) = get(&router, playlist_url).await.unwrap();
	assert_eq!(parts.status, StatusCode::NOT_FOUND);
	let (parts, _) = get(&router, &format!("/media/{id}/hls/0.ts"))
		.await
		.unwrap();
	assert_eq!(parts.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn files_outside_the_media_roots_are_refused() {
	let dir = tempfile::tempdir().unwrap();
	let other = tempfile::tempdir().unwrap();
	let clip = make_clip(other.path()).await.unwrap();
	let router = router(dir.path());

	// A native-dialog id is not accepted outside desktop mode
	let (status, json) = rpc(
		&router,
		START,
		serde_json::json!({ "fileId": absolute_id(&clip) }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
	assert_eq!(json["code"], "permission_denied");

	// A path is not an id, inside a root or not
	for not_an_id in [clip.display().to_string(), "clip.mkv".to_owned()] {
		let (status, json) = rpc(&router, START, serde_json::json!({ "fileId": not_an_id }))
			.await
			.unwrap();
		assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
		assert_eq!(json["code"], "invalid_argument");
	}

	let (status, json) = rpc(
		&router,
		START,
		serde_json::json!({ "fileId": id_of("missing.mkv") }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
	assert_eq!(json["code"], "not_found");

	// No media roots at all: the id names a root that is not configured.
	let bare = app(&Config {
		port: 0,
		..Config::default()
	});
	let (status, json) = rpc(
		&bare,
		START,
		serde_json::json!({ "fileId": id_of("clip.mkv") }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
	assert_eq!(json["code"], "not_found");

	let (parts, _) = get(&router, "/media/nope/hls/index.m3u8").await.unwrap();
	assert_eq!(parts.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn text_files_are_not_media() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(dir.path().join("notes.txt"), "not media").unwrap();
	let router = router(dir.path());
	let (status, json) = rpc(
		&router,
		START,
		serde_json::json!({ "fileId": id_of("notes.txt") }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
	assert_eq!(json["code"], "failed_precondition");
}

#[tokio::test]
async fn native_dialog_ids_are_accepted_in_desktop_mode_only() {
	let dir = tempfile::tempdir().unwrap();
	let clip = make_clip(dir.path()).await.unwrap();
	let desktop = app(&Config {
		address: IpAddr::V4(Ipv4Addr::LOCALHOST),
		port: 0,
		media_roots: vec![dir.path().to_path_buf()],
		desktop: true,
		..Config::default()
	});

	let (status, by_path) = rpc(
		&desktop,
		START,
		serde_json::json!({ "fileId": absolute_id(&clip) }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{by_path}");

	// The same file: the same session as its in-root id resolves to.
	let (status, by_id) = rpc(
		&desktop,
		START,
		serde_json::json!({ "fileId": id_of("clip.mkv") }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{by_id}");
	assert_eq!(by_id["mediaId"], by_path["mediaId"]);

	// The container refuses the native-dialog id even inside a root.
	let container = router(dir.path());
	let (status, json) = rpc(
		&container,
		START,
		serde_json::json!({ "fileId": absolute_id(&clip) }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
	assert_eq!(json["code"], "permission_denied");
}
