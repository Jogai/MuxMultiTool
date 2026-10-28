//! The settings methods of the system service: store, read back, survive a restart.

use std::sync::Arc;

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_server::{AppState, Config, app};
use tower::ServiceExt;

const GET: &str = "/rpc/muxmultitool.system.v1.SystemService/GetSettings";
const UPDATE: &str = "/rpc/muxmultitool.system.v1.SystemService/UpdateSettings";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn call(router: axum::Router, path: &str, body: &str) -> Result<serde_json::Value> {
	let request = Request::post(path)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_owned()))?;
	let response = router.oneshot(request).await?;
	assert_eq!(response.status(), StatusCode::OK);
	let body = response.into_body().collect().await?.to_bytes();
	Ok(serde_json::from_slice(&body)?)
}

fn catppuccin_auto() -> serde_json::Value {
	serde_json::json!({
		"family": "catppuccin",
		"mode": "THEME_MODE_AUTO",
		"lightVariant": "latte",
		"darkVariant": "mocha"
	})
}

#[tokio::test]
async fn settings_start_empty_and_keep_an_update() {
	let router = app(&Config::default());

	let empty = call(router.clone(), GET, "{}").await.unwrap();
	assert_eq!(empty, serde_json::json!({ "settings": {} }));

	let body = serde_json::json!({ "settings": { "theme": catppuccin_auto() } });
	let stored = call(router.clone(), UPDATE, &body.to_string())
		.await
		.unwrap();
	assert_eq!(stored["settings"]["theme"], catppuccin_auto());

	let read = call(router, GET, "{}").await.unwrap();
	assert_eq!(read["settings"]["theme"], catppuccin_auto());
}

#[tokio::test]
async fn an_update_without_a_theme_keeps_the_stored_one() {
	let router = app(&Config::default());
	let body = serde_json::json!({ "settings": { "theme": { "family": "nord", "mode": "THEME_MODE_SINGLE", "variant": "nord" } } });
	call(router.clone(), UPDATE, &body.to_string())
		.await
		.unwrap();

	let after = call(router, UPDATE, r#"{"settings":{}}"#).await.unwrap();
	assert_eq!(after["settings"]["theme"]["family"], "nord");
	assert_eq!(after["settings"]["theme"]["variant"], "nord");
}

#[tokio::test]
async fn settings_survive_a_restart_with_a_data_dir() {
	let dir = tempfile::tempdir().unwrap();
	let config = Config {
		data_dir: Some(dir.path().join("data")),
		..Config::default()
	};

	let body = serde_json::json!({ "settings": { "theme": catppuccin_auto() } });
	call(app(&config), UPDATE, &body.to_string()).await.unwrap();
	assert!(dir.path().join("data/settings.json").is_file());

	// A new server over the same folder: what a container restart does
	let read = call(app(&config), GET, "{}").await.unwrap();
	assert_eq!(read["settings"]["theme"], catppuccin_auto());

	// And the state knows where it writes
	let state = AppState::new(&config);
	assert_eq!(
		state.settings.path(),
		Some(dir.path().join("data/settings.json").as_path())
	);
	drop(Arc::new(state));
}
