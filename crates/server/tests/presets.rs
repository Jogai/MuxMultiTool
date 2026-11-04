//! Presets end to end over the Connect router (JSON): create, list, rename,
//! update and delete, persistence under the data folder across a restart,
//! a file of an unknown version being reported, and applying a preset to a
//! file or a batch giving the same plan as the hand-entered settings.
//! Needs ffmpeg and ffprobe on `PATH`.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::time::Duration;

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner};
use muxmultitool_server::{Config, app};
use serde_json::{Value, json};
use tower::ServiceExt;

const LIST: &str = "/rpc/muxmultitool.presets.v1.PresetsService/ListPresets";
const CREATE: &str = "/rpc/muxmultitool.presets.v1.PresetsService/CreatePreset";
const UPDATE: &str = "/rpc/muxmultitool.presets.v1.PresetsService/UpdatePreset";
const RENAME: &str = "/rpc/muxmultitool.presets.v1.PresetsService/RenamePreset";
const DELETE: &str = "/rpc/muxmultitool.presets.v1.PresetsService/DeletePreset";
const APPLY_FILE: &str = "/rpc/muxmultitool.presets.v1.PresetsService/ApplyPresetToFile";
const APPLY_BATCH: &str = "/rpc/muxmultitool.presets.v1.PresetsService/ApplyPresetToBatch";
const PREVIEW: &str = "/rpc/muxmultitool.streams.v1.StreamsService/PreviewStreams";
const PLAN: &str = "/rpc/muxmultitool.batch.v1.BatchService/PlanBatch";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn post_json(router: &axum::Router, path: &str, body: Value) -> Result<(StatusCode, Value)> {
	let request = Request::post(path)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_string()))?;
	let response = router.clone().oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes();
	Ok((parts.status, serde_json::from_slice(&body)?))
}

/// An OK response's body.
async fn ok(router: &axum::Router, path: &str, body: Value) -> Result<Value> {
	let (status, json) = post_json(router, path, body).await?;
	assert_eq!(status, StatusCode::OK, "{path}: {json}");
	Ok(json)
}

fn config(root: &Path, data: Option<&Path>) -> Config {
	Config {
		address: IpAddr::V4(Ipv4Addr::LOCALHOST),
		port: 0,
		media_roots: vec![root.to_path_buf()],
		frontend_dir: root.join("no-frontend"),
		data_dir: data.map(Path::to_path_buf),
		..Config::default()
	}
}

/// `in/clip.mp4` and `in/second.mp4`: H.264 and AAC in English and French.
async fn media(root: &Path) -> Result<()> {
	let input = root.join("in");
	std::fs::create_dir_all(&input)?;
	std::fs::create_dir(root.join("out"))?;
	let clip = input.join("clip.mp4");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=duration=1:size=64x64:rate=10")
		.option("-f", "lavfi")
		.input("sine=frequency=440:duration=1")
		.option("-f", "lavfi")
		.input("sine=frequency=880:duration=1")
		.args(["-map", "0", "-map", "1", "-map", "2"])
		.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
		.option("-metadata:s:a:0", "language=eng")
		.option("-metadata:s:a:1", "language=fre")
		.output(&clip);
	Runner::default()
		.run(&cmd, &Limits::timeout(Duration::from_secs(60)), None)
		.await?;
	std::fs::copy(&clip, input.join("second.mp4"))?;
	Ok(())
}

fn english_only() -> Value {
	json!({
		"container": "PRESET_CONTAINER_MKV",
		"template": "{name}.{lang}",
		"rules": {
			"rules": [{
				"action": "RULE_ACTION_KEEP",
				"streamType": "STREAM_TYPE_AUDIO",
				"language": "eng",
			}],
			"untagged": "UNTAGGED_LANGUAGE_DROP",
		},
	})
}

#[tokio::test]
async fn presets_are_managed_and_survive_a_restart() -> Result<()> {
	let root = tempfile::tempdir().unwrap();
	let data = root.path().join("data");
	let router = app(&config(root.path(), Some(&data)));

	assert_eq!(ok(&router, LIST, json!({})).await?, json!({}));
	let created = ok(
		&router,
		CREATE,
		json!({ "name": "English only", "settings": english_only() }),
	)
	.await?;
	let id = created["preset"]["id"].as_str().unwrap().to_owned();
	assert_eq!(created["preset"]["name"], "English only");
	assert_eq!(created["preset"]["settings"], english_only());

	// Errors: a duplicate name, a bad template, no container, unknown id.
	let (status, _) = post_json(
		&router,
		CREATE,
		json!({ "name": "english ONLY", "settings": english_only() }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::CONFLICT);
	let mut bad = english_only();
	bad["template"] = json!("{nope}.x");
	let (status, _) = post_json(&router, CREATE, json!({ "name": "b", "settings": bad }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST);
	let mut bad = english_only();
	bad["container"] = json!("PRESET_CONTAINER_UNSPECIFIED");
	let (status, _) = post_json(&router, CREATE, json!({ "name": "b", "settings": bad }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST);
	let (status, _) = post_json(&router, RENAME, json!({ "id": "99", "name": "x" }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::NOT_FOUND);

	let renamed = ok(&router, RENAME, json!({ "id": id, "name": "EN" })).await?;
	assert_eq!(renamed["preset"]["name"], "EN");
	let mut settings = english_only();
	settings["container"] = json!("PRESET_CONTAINER_MP4");
	let updated = ok(&router, UPDATE, json!({ "id": id, "settings": settings })).await?;
	assert_eq!(updated["preset"]["name"], "EN");
	assert_eq!(
		updated["preset"]["settings"]["container"],
		"PRESET_CONTAINER_MP4"
	);

	// A new server on the same data folder has the preset.
	drop(router);
	let router = app(&config(root.path(), Some(&data)));
	let listed = ok(&router, LIST, json!({})).await?;
	assert_eq!(listed["presets"], json!([updated["preset"]]));

	ok(&router, DELETE, json!({ "id": id })).await?;
	assert_eq!(ok(&router, LIST, json!({})).await?, json!({}));
	Ok(())
}

#[tokio::test]
async fn a_file_of_a_newer_version_is_reported_and_kept() -> Result<()> {
	let root = tempfile::tempdir().unwrap();
	let data = root.path().join("data");
	std::fs::create_dir(&data).unwrap();
	let file = data.join("presets.json");
	let content = r#"{"version":9,"presets":[]}"#;
	std::fs::write(&file, content).unwrap();
	let router = app(&config(root.path(), Some(&data)));

	let (status, error) = post_json(&router, LIST, json!({})).await.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
	assert_eq!(error["code"], "failed_precondition");
	assert!(
		error["message"].as_str().unwrap().contains("newer version"),
		"{error}"
	);
	let (status, _) = post_json(
		&router,
		CREATE,
		json!({ "name": "x", "settings": english_only() }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST);
	assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
	Ok(())
}

#[tokio::test]
async fn applying_a_preset_plans_like_the_hand_entered_settings() -> Result<()> {
	let root = tempfile::tempdir().unwrap();
	media(root.path()).await.unwrap();
	let router = app(&config(root.path(), None));
	let created = ok(
		&router,
		CREATE,
		json!({ "name": "English only", "settings": english_only() }),
	)
	.await?;
	let id = created["preset"]["id"].as_str().unwrap();
	let input = root.path().join("in");
	let out = root.path().join("out");

	// A batch.
	let applied = ok(
		&router,
		APPLY_BATCH,
		json!({
			"presetId": id,
			"folder": input,
			"patterns": ["*.mp4"],
			"outputFolder": out,
		}),
	)
	.await?;
	let by_hand = json!({
		"folder": input,
		"patterns": ["*.mp4"],
		"outputFolder": out,
		"template": "{name}.{lang}.mkv",
		"mode": "SELECTION_MODE_RULES",
		"rules": english_only()["rules"],
	});
	let from_preset = ok(&router, PLAN, json!({ "operation": applied["operation"] })).await?;
	let from_hand = ok(&router, PLAN, json!({ "operation": by_hand })).await?;
	assert_eq!(from_preset, from_hand);
	assert_eq!(from_preset["ready"], 2, "{from_preset}");
	let item = &from_preset["items"][0];
	assert_eq!(item["outputName"], "clip.eng.mkv");
	let kept: Vec<_> = item["streams"]
		.as_array()
		.unwrap()
		.iter()
		.map(|s| s["kept"] == true)
		.collect();
	assert_eq!(kept, [true, true, false]);

	// A single file.
	let applied = ok(
		&router,
		APPLY_FILE,
		json!({ "presetId": id, "input": input.join("clip.mp4") }),
	)
	.await?;
	assert_eq!(applied["operation"]["output"], "clip.eng.mkv");
	let by_hand = json!({
		"input": input.join("clip.mp4"),
		"output": "clip.eng.mkv",
		"mode": "SELECTION_MODE_RULES",
		"rules": english_only()["rules"],
	});
	let from_preset = ok(
		&router,
		PREVIEW,
		json!({ "operation": applied["operation"] }),
	)
	.await?;
	let from_hand = ok(&router, PREVIEW, json!({ "operation": by_hand })).await?;
	assert_eq!(from_preset, from_hand);
	assert_eq!(from_preset["container"], "MKV");
	assert_eq!(from_preset["canRun"], true);

	// An unknown preset.
	let (status, _) = post_json(
		&router,
		APPLY_FILE,
		json!({ "presetId": "99", "input": input.join("clip.mp4") }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::NOT_FOUND);
	Ok(())
}
