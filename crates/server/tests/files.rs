//! The files service over HTTP: browsing, wildcards, opaque ids and the
//! container/desktop difference in what a client may send.

use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_server::{Config, app};
use serde_json::{Value, json};
use tower::ServiceExt;

const SERVICE: &str = "/rpc/muxmultitool.files.v1.FilesService";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root next to a folder that must stay out of reach:
///
/// ```text
/// media/
///   a.mp4, b.mkv, notes.txt
///   series/ s01e01.mkv, s01e02.mkv
///   escape.mp4 -> ../outside/secret.mp4   (Unix only)
/// outside/
///   secret.mp4
/// ```
struct Fixture {
	dir: tempfile::TempDir,
}

impl Fixture {
	fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let media = dir.path().join("media");
		fs::create_dir_all(media.join("series"))?;
		fs::create_dir_all(dir.path().join("outside"))?;
		fs::write(media.join("a.mp4"), b"abc")?;
		fs::write(media.join("b.mkv"), b"")?;
		fs::write(media.join("notes.txt"), b"")?;
		fs::write(media.join("series/s01e01.mkv"), b"")?;
		fs::write(media.join("series/s01e02.mkv"), b"")?;
		fs::write(dir.path().join("outside/secret.mp4"), b"")?;
		#[cfg(unix)]
		std::os::unix::fs::symlink(
			dir.path().join("outside/secret.mp4"),
			media.join("escape.mp4"),
		)?;
		Ok(Self { dir })
	}

	fn media(&self) -> PathBuf {
		self.dir.path().join("media")
	}

	fn outside(&self) -> PathBuf {
		self.dir.path().join("outside")
	}

	fn config(&self, desktop: bool) -> Config {
		Config {
			address: IpAddr::V4(Ipv4Addr::LOCALHOST),
			port: 0,
			media_roots: vec![self.media()],
			frontend_dir: self.dir.path().join("no-frontend"),
			desktop,
			..Config::default()
		}
	}
}

/// Calls `method` with a JSON body and returns the status and the JSON answer.
async fn call(config: &Config, method: &str, body: Value) -> Result<(StatusCode, Value)> {
	let request = Request::post(format!("{SERVICE}/{method}"))
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_string()))?;
	let response = app(config).oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes();
	Ok((parts.status, serde_json::from_slice(&body)?))
}

fn names(entries: &Value) -> Vec<&str> {
	entries
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|e| e["name"].as_str())
		.collect()
}

/// The id of the entry called `name` in a listing.
fn id_of<'a>(entries: &'a Value, name: &str) -> Option<&'a str> {
	entries.as_array()?.iter().find(|e| e["name"] == name)?["id"].as_str()
}

/// Lists the roots and returns the id of the one media root.
async fn root_id(config: &Config) -> Result<String> {
	let (status, body) = call(config, "ListFolder", json!({})).await?;
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(names(&body["entries"]), ["media"]);
	assert!(body.get("folder").is_none(), "{body}");
	Ok(id_of(&body["entries"], "media")
		.ok_or("no media root")?
		.to_owned())
}

#[tokio::test]
async fn lists_the_roots_and_the_folders_under_them() {
	let fixture = Fixture::new().unwrap();
	let config = fixture.config(false);
	let root = root_id(&config).await.unwrap();

	let (status, body) = call(&config, "ListFolder", json!({ "folderId": root }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(body["folder"]["name"], "media");
	assert_eq!(body["folder"]["kind"], "ENTRY_KIND_FOLDER");
	assert_eq!(body["folder"]["id"], root);
	assert!(body.get("ancestors").is_none(), "{body}");
	// Folders first, media files after; notes.txt and the escaping symlink are not there
	assert_eq!(names(&body["entries"]), ["series", "a.mp4", "b.mkv"]);
	let a = &body["entries"][1];
	assert_eq!(a["kind"], "ENTRY_KIND_FILE");
	assert_eq!(a["sizeBytes"], "3");
	assert_eq!(a["path"], "a.mp4");
	// An id is opaque: no path in it
	assert!(!a["id"].as_str().unwrap().contains("a.mp4"), "{}", a["id"]);

	let series = id_of(&body["entries"], "series").unwrap();
	let (status, body) = call(&config, "ListFolder", json!({ "folderId": series }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(names(&body["ancestors"]), ["media"]);
	assert_eq!(body["ancestors"][0]["id"], root);
	assert_eq!(body["folder"]["path"], "series");
	assert_eq!(names(&body["entries"]), ["s01e01.mkv", "s01e02.mkv"]);
	assert_eq!(body["entries"][0]["path"], "series/s01e01.mkv");
}

#[tokio::test]
async fn finds_files_by_wildcard() {
	let fixture = Fixture::new().unwrap();
	let config = fixture.config(false);
	let root = root_id(&config).await.unwrap();

	let (status, body) = call(
		&config,
		"FindFiles",
		json!({ "folderId": root, "patterns": ["*.mp4"] }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(names(&body["files"]), ["a.mp4"]);

	// Several patterns, any case, through the subfolders
	let (status, body) = call(
		&config,
		"FindFiles",
		json!({ "folderId": root, "patterns": ["*.MP4", "S01E0?.mkv"], "recursive": true }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(
		body["files"]
			.as_array()
			.unwrap()
			.iter()
			.map(|f| f["path"].as_str().unwrap())
			.collect::<Vec<_>>(),
		["a.mp4", "series/s01e01.mkv", "series/s01e02.mkv"]
	);

	let (status, body) = call(&config, "FindFiles", json!({ "folderId": root }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	assert_eq!(body["code"], "invalid_argument");

	let (status, body) = call(
		&config,
		"FindFiles",
		json!({ "folderId": root, "patterns": ["[oops"] }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	assert_eq!(body["code"], "invalid_argument");
	assert!(
		body["message"].as_str().unwrap().contains("pattern"),
		"{body}"
	);
}

#[tokio::test]
async fn ids_cannot_reach_outside_the_roots() {
	let fixture = Fixture::new().unwrap();
	let config = fixture.config(false);
	let root = root_id(&config).await.unwrap();

	// Ids are the server's: a made-up one is an invalid argument, not a path lookup
	let (status, body) = call(&config, "ListFolder", json!({ "folderId": "../outside" }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	assert_eq!(body["code"], "invalid_argument");

	let (status, body) = call(&config, "ListFolder", json!({ "folderId": "/etc" }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	assert_eq!(body["code"], "invalid_argument");

	// A crafted id in the server's own encoding with `..` or an absolute path
	let encode = |text: &str| {
		base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, text)
	};
	for text in ["r0:../outside", "r0:/etc", "r0:series/../../outside"] {
		let (status, body) = call(&config, "ListFolder", json!({ "folderId": encode(text) }))
			.await
			.unwrap();
		assert_eq!(status, StatusCode::BAD_REQUEST, "{text}: {body}");
		assert_eq!(body["code"], "invalid_argument", "{text}");
	}

	// A root index that is not configured
	let (status, body) = call(&config, "ListFolder", json!({ "folderId": encode("r9:") }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
	assert_eq!(body["code"], "not_found");

	// An absolute-path id is only for desktop mode, whatever it points at
	let inside = encode(&format!("a:{}", fixture.media().join("a.mp4").display()));
	let (status, body) = call(&config, "GetEntries", json!({ "ids": [inside] }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(body["entries"][0]["kind"], Value::Null, "{body}");
	let (status, body) = call(
		&config,
		"ListFolder",
		json!({ "folderId": encode(&format!("a:{}", fixture.media().display())) }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
	assert_eq!(body["code"], "permission_denied");

	// A symlink that leaves the root: not listed (above), and its id is refused
	#[cfg(unix)]
	{
		let escape = encode("r0:escape.mp4");
		let (status, body) = call(&config, "GetEntries", json!({ "ids": [&escape, &root] }))
			.await
			.unwrap();
		assert_eq!(status, StatusCode::OK, "{body}");
		assert_eq!(body["entries"][0], json!({ "id": escape }), "{body}");
		assert_eq!(body["entries"][1]["name"], "media");

		let (status, body) = call(
			&config,
			"FindFiles",
			json!({ "folderId": escape, "patterns": ["*"] }),
		)
		.await
		.unwrap();
		assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
		assert_eq!(body["code"], "permission_denied");
	}
}

#[tokio::test]
async fn client_paths_are_refused_in_the_container() {
	let fixture = Fixture::new().unwrap();
	let config = fixture.config(false);
	let path = fixture.media().join("a.mp4");

	let (status, body) = call(&config, "OpenPaths", json!({ "paths": [path] }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
	assert_eq!(body["code"], "permission_denied");
	assert_eq!(
		body["message"],
		"paths from the client are only accepted in desktop mode"
	);
}

#[tokio::test]
async fn client_paths_are_accepted_on_the_desktop() {
	let fixture = Fixture::new().unwrap();
	let config = fixture.config(true);
	let file = fixture.outside().join("secret.mp4");
	let folder = fixture.media();

	let (status, body) = call(&config, "OpenPaths", json!({ "paths": [&file, &folder] }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(names(&body["entries"]), ["secret.mp4", "media"]);
	assert_eq!(body["entries"][0]["kind"], "ENTRY_KIND_FILE");
	assert_eq!(body["entries"][0]["path"], file.to_str().unwrap());
	assert_eq!(body["entries"][1]["kind"], "ENTRY_KIND_FOLDER");

	// The folder from the dialog can be browsed and searched like a root
	let folder_id = id_of(&body["entries"], "media").unwrap().to_owned();
	let (status, body) = call(&config, "ListFolder", json!({ "folderId": folder_id }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	// No root to escape from: the symlink is an ordinary file here
	#[cfg(unix)]
	assert_eq!(
		names(&body["entries"]),
		["series", "a.mp4", "b.mkv", "escape.mp4"]
	);
	#[cfg(not(unix))]
	assert_eq!(names(&body["entries"]), ["series", "a.mp4", "b.mkv"]);
	let (status, body) = call(
		&config,
		"FindFiles",
		json!({ "folderId": folder_id, "patterns": ["*.mkv"], "recursive": true }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(names(&body["files"]), ["b.mkv", "s01e01.mkv", "s01e02.mkv"]);

	// Relative and missing paths are still errors
	let (status, body) = call(&config, "OpenPaths", json!({ "paths": ["relative.mp4"] }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	assert_eq!(body["code"], "invalid_argument");
	let (status, body) = call(
		&config,
		"OpenPaths",
		json!({ "paths": [Path::new("/").join("no-such-file.mp4")] }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
	assert_eq!(body["code"], "not_found");
}

#[tokio::test]
async fn get_entries_answers_every_id_in_order() {
	let fixture = Fixture::new().unwrap();
	let config = fixture.config(false);
	let root = root_id(&config).await.unwrap();
	let (_, listing) = call(&config, "ListFolder", json!({ "folderId": root }))
		.await
		.unwrap();
	let a = id_of(&listing["entries"], "a.mp4").unwrap().to_owned();
	let series = id_of(&listing["entries"], "series").unwrap().to_owned();

	fs::remove_file(fixture.media().join("a.mp4")).unwrap();
	let (status, body) = call(
		&config,
		"GetEntries",
		json!({ "ids": [&series, &a, "garbage"] }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	let entries = body["entries"].as_array().unwrap();
	assert_eq!(entries.len(), 3);
	assert_eq!(entries[0]["name"], "series");
	assert_eq!(entries[0]["kind"], "ENTRY_KIND_FOLDER");
	// Deleted since the id was handed out: the id alone, kind unspecified
	assert_eq!(entries[1], json!({ "id": a }));
	assert_eq!(entries[2], json!({ "id": "garbage" }));
}
