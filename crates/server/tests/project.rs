//! The project methods of the cut service over the Connect router, as the
//! dev frontend uses them (JSON): save and restore per source file, undo
//! and redo, a restart with a data folder, a changed or missing source,
//! and the request validation. No ffmpeg needed: a project never probes.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::files::Location;
use muxmultitool_server::{Config, app};
use tower::ServiceExt;

const GET: &str = "/rpc/muxmultitool.cut.v1.CutService/GetProject";
const UPDATE: &str = "/rpc/muxmultitool.cut.v1.CutService/UpdateProject";
const UNDO: &str = "/rpc/muxmultitool.cut.v1.CutService/UndoProject";
const REDO: &str = "/rpc/muxmultitool.cut.v1.CutService/RedoProject";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with a (fake) clip in it, and a data folder next to it.
struct Fixture {
	dir: tempfile::TempDir,
	clip: PathBuf,
}

impl Fixture {
	fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let root = dir.path().join("media");
		std::fs::create_dir_all(root.join("sub"))?;
		let clip = root.join("clip.mkv");
		std::fs::write(&clip, b"not really a video, but a project never looks")?;
		Ok(Self { dir, clip })
	}

	fn root(&self) -> PathBuf {
		self.dir.path().join("media")
	}

	fn data_dir(&self) -> PathBuf {
		self.dir.path().join("data")
	}

	fn config(&self) -> Config {
		Config {
			address: IpAddr::V4(Ipv4Addr::LOCALHOST),
			port: 0,
			media_roots: vec![self.root()],
			frontend_dir: self.dir.path().join("no-frontend"),
			data_dir: Some(self.data_dir()),
			..Config::default()
		}
	}

	/// The id `FilesService` would give the clip.
	fn clip_id() -> String {
		Location::InRoot {
			root: 0,
			relative: PathBuf::from("clip.mkv"),
		}
		.id()
		.to_string()
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

/// Calls `path` and returns the `project` of an OK response.
async fn project(
	router: &axum::Router,
	path: &str,
	body: serde_json::Value,
) -> Result<serde_json::Value> {
	let (status, json) = post_json(router, path, body).await?;
	if status != StatusCode::OK {
		return Err(format!("{path} refused: {json}").into());
	}
	Ok(json["project"].clone())
}

fn segments(ranges: &[(f64, f64)]) -> serde_json::Value {
	serde_json::Value::Array(
		ranges
			.iter()
			.map(|(s, e)| serde_json::json!({ "startSeconds": s, "endSeconds": e }))
			.collect(),
	)
}

/// A state as a client sends it: the modes left at their defaults.
fn state(ranges: &[(f64, f64)]) -> serde_json::Value {
	serde_json::json!({ "segments": segments(ranges) })
}

/// The same state as the server returns it: every mode spelled out.
fn stored(ranges: &[(f64, f64)]) -> serde_json::Value {
	let mut json = serde_json::json!({
		"segmentMode": "SEGMENT_MODE_KEEP",
		"cutMode": "CUT_MODE_KEYFRAME",
		"export": "CUT_EXPORT_SEPARATE_FILES"
	});
	if !ranges.is_empty() {
		json["segments"] = segments(ranges);
	}
	json
}

#[tokio::test]
async fn a_project_is_saved_per_file_restored_after_a_restart_and_undone() -> Result<()> {
	let fixture = Fixture::new()?;
	let router = app(&fixture.config());
	let id = Fixture::clip_id();

	// Nothing yet: an empty project, with the file as the server sees it.
	let empty = project(&router, GET, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(empty["saved"], serde_json::Value::Null, "{empty}");
	assert_eq!(empty["state"], stored(&[]), "{empty}");
	assert_eq!(empty["sourceStatus"], "SOURCE_STATUS_UNCHANGED");
	assert_eq!(
		Path::new(empty["sourcePath"].as_str().unwrap_or_default()),
		fixture.clip.canonicalize()?
	);

	// Two changes, the second with every mode set.
	let first = state(&[(1.0, 2.0)]);
	let second = serde_json::json!({
		"segments": segments(&[(1.0, 2.0), (3.5, 4.25)]),
		"segmentMode": "SEGMENT_MODE_REMOVE",
		"cutMode": "CUT_MODE_NORMAL",
		"export": "CUT_EXPORT_MERGED"
	});
	let saved = project(
		&router,
		UPDATE,
		serde_json::json!({ "fileId": id, "state": first }),
	)
	.await?;
	assert_eq!(saved["saved"], true);
	assert_eq!(
		(saved["undoDepth"].as_u64(), saved["redoDepth"].as_u64()),
		(Some(1), None)
	);
	let saved = project(
		&router,
		UPDATE,
		serde_json::json!({ "fileId": id, "state": second }),
	)
	.await?;
	assert_eq!(saved["state"], second, "{saved}");
	assert_eq!(saved["undoDepth"], 2);

	// The project file is in the data folder, not next to the clip.
	let projects = fixture.data_dir().join("projects");
	let files: Vec<_> = std::fs::read_dir(&projects)?.flatten().collect();
	assert_eq!(files.len(), 1, "{files:?}");
	assert!(files[0].file_name().to_string_lossy().starts_with("clip-"));
	assert_eq!(std::fs::read_dir(fixture.root())?.count(), 2);

	// The same file by absolute path is the same project.
	let by_path = project(&router, GET, serde_json::json!({ "fileId": fixture.clip })).await?;
	assert_eq!(by_path["state"], second);

	// Undo, undo, redo.
	let undone = project(&router, UNDO, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(undone["state"], stored(&[(1.0, 2.0)]), "{undone}");
	assert_eq!(undone["undoDepth"], 1, "{undone}");
	assert_eq!(undone["redoDepth"], 1, "{undone}");
	let undone = project(&router, UNDO, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(undone["state"], stored(&[]), "{undone}");
	let (status, json) = post_json(&router, UNDO, serde_json::json!({ "fileId": id })).await?;
	assert_ne!(status, StatusCode::OK);
	assert_eq!(json["code"], "failed_precondition", "{json}");
	let redone = project(&router, REDO, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(redone["state"], stored(&[(1.0, 2.0)]), "{redone}");

	// A restart (a new server over the same data folder) restores the
	// state on disk, without the history.
	let restarted = app(&fixture.config());
	let restored = project(&restarted, GET, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(restored["saved"], true);
	assert_eq!(restored["state"], stored(&[(1.0, 2.0)]), "{restored}");
	assert_eq!(restored["undoDepth"], serde_json::Value::Null);
	assert_eq!(restored["sourceStatus"], "SOURCE_STATUS_UNCHANGED");
	let (status, json) = post_json(&restarted, REDO, serde_json::json!({ "fileId": id })).await?;
	assert_ne!(status, StatusCode::OK);
	assert_eq!(json["code"], "failed_precondition", "{json}");

	// Another file is another project.
	let other = project(
		&restarted,
		GET,
		serde_json::json!({ "fileId": fixture.root().join("sub/other.mkv") }),
	)
	.await?;
	assert_eq!(other["saved"], serde_json::Value::Null, "{other}");
	Ok(())
}

#[tokio::test]
async fn a_changed_or_missing_source_is_reported_with_its_segments() -> Result<()> {
	let fixture = Fixture::new()?;
	let router = app(&fixture.config());
	let id = Fixture::clip_id();
	project(
		&router,
		UPDATE,
		serde_json::json!({ "fileId": id, "state": state(&[(10.0, 20.0)]) }),
	)
	.await?;
	let marked = stored(&[(10.0, 20.0)]);

	// Touched: same size, other modification time.
	let file = std::fs::File::options().write(true).open(&fixture.clip)?;
	file.set_modified(SystemTime::now() + Duration::from_secs(120))?;
	drop(file);
	let changed = project(&router, GET, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(
		changed["sourceStatus"], "SOURCE_STATUS_CHANGED",
		"{changed}"
	);
	assert_eq!(changed["state"], marked);

	// Gone: the id still names its place, the segments are still there,
	// and the project can still be edited.
	std::fs::remove_file(&fixture.clip)?;
	let missing = project(&router, GET, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(
		missing["sourceStatus"], "SOURCE_STATUS_MISSING",
		"{missing}"
	);
	assert_eq!(missing["state"], marked);
	let missing = project(&router, UNDO, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(
		missing["sourceStatus"], "SOURCE_STATUS_MISSING",
		"{missing}"
	);
	assert_eq!(missing["state"], stored(&[]));

	// Back, with other contents: changed against the last save.
	std::fs::write(&fixture.clip, b"a longer file than before, by a few words")?;
	let back = project(&router, GET, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(back["sourceStatus"], "SOURCE_STATUS_CHANGED", "{back}");
	Ok(())
}

#[tokio::test]
async fn without_a_data_folder_projects_last_for_the_run() -> Result<()> {
	let fixture = Fixture::new()?;
	let config = Config {
		data_dir: None,
		..fixture.config()
	};
	let router = app(&config);
	let id = Fixture::clip_id();
	project(
		&router,
		UPDATE,
		serde_json::json!({ "fileId": id, "state": state(&[(1.0, 2.0)]) }),
	)
	.await?;
	let read = project(&router, GET, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(read["state"], stored(&[(1.0, 2.0)]));
	assert!(!fixture.data_dir().exists());

	let restarted = project(&app(&config), GET, serde_json::json!({ "fileId": id })).await?;
	assert_eq!(restarted["saved"], serde_json::Value::Null, "{restarted}");
	Ok(())
}

#[tokio::test]
async fn requests_are_checked() -> Result<()> {
	let fixture = Fixture::new()?;
	let router = app(&fixture.config());
	let outside = tempfile::tempdir()?;
	let folder_id = Location::InRoot {
		root: 0,
		relative: PathBuf::from("sub"),
	}
	.id()
	.to_string();
	let desktop_id = Location::Absolute(fixture.clip.clone()).id().to_string();
	let cases = [
		(GET, serde_json::json!({}), "invalid_argument"),
		(
			GET,
			serde_json::json!({ "fileId": "relative/clip.mkv" }),
			"invalid_argument",
		),
		(
			GET,
			serde_json::json!({ "fileId": folder_id }),
			"invalid_argument",
		),
		(
			GET,
			serde_json::json!({ "fileId": fixture.root().join("sub") }),
			"invalid_argument",
		),
		// A root's own parent is outside the roots.
		(
			GET,
			serde_json::json!({ "fileId": fixture.root() }),
			"permission_denied",
		),
		(
			GET,
			serde_json::json!({ "fileId": outside.path().join("clip.mkv") }),
			"permission_denied",
		),
		(
			GET,
			serde_json::json!({ "fileId": fixture.root().join("nowhere/clip.mkv") }),
			"not_found",
		),
		// A native-dialog path is only trusted in desktop mode.
		(
			GET,
			serde_json::json!({ "fileId": desktop_id }),
			"permission_denied",
		),
		(
			UPDATE,
			serde_json::json!({ "fileId": fixture.clip, "state": state(&[(-1.0, 2.0)]) }),
			"invalid_argument",
		),
		(
			UPDATE,
			serde_json::json!({ "fileId": fixture.clip, "state": { "segmentMode": 7 } }),
			"invalid_argument",
		),
	];
	for (path, body, code) in cases {
		let (status, json) = post_json(&router, path, body.clone()).await?;
		assert_ne!(status, StatusCode::OK, "{body}: {json}");
		assert_eq!(json["code"], code, "{body}: {json}");
	}

	// Nothing was saved by the refused requests.
	assert!(!fixture.data_dir().exists());
	Ok(())
}
