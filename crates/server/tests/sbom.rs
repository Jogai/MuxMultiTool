//! `GET /sbom.cdx.json`: the `CycloneDX` document the server serves.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::Tools;
use muxmultitool_server::{Config, app};
use serde_json::Value;
use tower::ServiceExt;

const PATH: &str = "/sbom.cdx.json";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A frontend folder with an `index.html`, optionally with the npm package
/// list `vite build` writes.
fn frontend_dir(web_sbom: Option<&str>) -> Result<tempfile::TempDir> {
	let dir = tempfile::tempdir()?;
	std::fs::write(
		dir.path().join("index.html"),
		"<!doctype html><title>mmt</title>",
	)?;
	if let Some(json) = web_sbom {
		std::fs::write(dir.path().join("sbom.web.json"), json)?;
	}
	Ok(dir)
}

fn config(frontend_dir: &Path, tools: Tools) -> Config {
	Config {
		address: IpAddr::V4(Ipv4Addr::LOCALHOST),
		port: 0,
		frontend_dir: frontend_dir.to_path_buf(),
		tools,
		..Config::default()
	}
}

/// Fetches the document from the router in process, without a socket.
async fn fetch(web_sbom: Option<&str>, tools: Tools) -> Result<(http::response::Parts, Value)> {
	let dir = frontend_dir(web_sbom)?;
	let request = Request::get(PATH).body(Body::empty())?;
	let response = app(&config(dir.path(), tools)).oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes();
	Ok((parts, serde_json::from_slice(&body)?))
}

/// Tools that do not exist, so the document has no ffmpeg component.
fn no_ffmpeg() -> Tools {
	Tools {
		ffmpeg: PathBuf::from("/nonexistent/ffmpeg"),
		ffprobe: PathBuf::from("/nonexistent/ffprobe"),
	}
}

fn components<'a>(document: &'a Value, kind: &str) -> Vec<&'a Value> {
	document["components"]
		.as_array()
		.into_iter()
		.flatten()
		.filter(|c| c["type"] == kind)
		.collect()
}

fn by_name<'a>(document: &'a Value, name: &str) -> Option<&'a Value> {
	document["components"]
		.as_array()
		.into_iter()
		.flatten()
		.find(|c| c["name"] == name)
}

#[tokio::test]
async fn the_document_is_cyclonedx_1_6() {
	let (parts, document) = fetch(None, no_ffmpeg()).await.unwrap();

	assert_eq!(parts.status, StatusCode::OK);
	assert!(
		parts.headers[header::CONTENT_TYPE]
			.to_str()
			.unwrap()
			.starts_with("application/vnd.cyclonedx+json")
	);
	assert_eq!(document["bomFormat"], "CycloneDX");
	assert_eq!(document["specVersion"], "1.6");
	assert!(
		document["serialNumber"]
			.as_str()
			.unwrap()
			.starts_with("urn:uuid:")
	);
	assert_eq!(document["version"], 1);
	// The build time, RFC 3339
	let timestamp = document["metadata"]["timestamp"].as_str().unwrap();
	assert!(
		timestamp.ends_with('Z') && timestamp.contains('T'),
		"{timestamp}"
	);
	let application = &document["metadata"]["component"];
	assert_eq!(application["type"], "application");
	assert_eq!(application["name"], "muxmultitool");
	assert_eq!(application["version"], muxmultitool_server::VERSION);
	assert_eq!(application["licenses"][0]["expression"], "GPL-3.0-or-later");
}

#[tokio::test]
async fn the_crates_are_the_normal_dependencies_of_the_server() {
	let (_, document) = fetch(None, no_ffmpeg()).await.unwrap();

	// A dependency of the core crate, through the server
	let tokio = by_name(&document, "tokio").expect("tokio is compiled in");
	assert_eq!(tokio["type"], "library");
	let version = tokio["version"].as_str().unwrap();
	assert!(
		version.starts_with(|c: char| c.is_ascii_digit()),
		"{version}"
	);
	assert_eq!(tokio["purl"], format!("pkg:cargo/tokio@{version}"));
	assert_eq!(tokio["licenses"][0]["expression"], "MIT");
	assert_eq!(
		tokio["externalReferences"][0]["url"],
		"https://github.com/tokio-rs/tokio"
	);
	// The server's own direct dependency
	assert!(by_name(&document, "axum").is_some());
	// Dev-only crates of the workspace are not compiled in
	for dev_only in ["insta", "proptest"] {
		assert!(
			by_name(&document, dev_only).is_none(),
			"{dev_only} is dev-only"
		);
	}
	// The workspace crates are the application, not components
	for own in [
		"muxmultitool-core",
		"muxmultitool-proto",
		"muxmultitool-server",
	] {
		assert!(by_name(&document, own).is_none(), "{own}");
	}
	// Every crate has a cargo purl and the root depends on every component
	let crates = components(&document, "library");
	assert!(crates.len() > 50, "{}", crates.len());
	assert!(
		crates
			.iter()
			.all(|c| c["purl"].as_str().unwrap().starts_with("pkg:cargo/"))
	);
	let depends_on = document["dependencies"][0]["dependsOn"].as_array().unwrap();
	assert_eq!(depends_on.len(), crates.len());
}

#[tokio::test]
async fn proc_macros_are_build_time_and_everything_else_runtime() {
	let web = r#"{"packages":[{"name":"vue","version":"3.5.45","license":"MIT"}]}"#;
	let (_, document) = fetch(Some(web), no_ffmpeg()).await.unwrap();

	let scope = |name: &str| by_name(&document, name).map(|c| c["scope"].clone());
	// Linked into the server
	assert_eq!(scope("tokio"), Some("required".into()));
	assert_eq!(scope("axum"), Some("required".into()));
	// Procedural macros run inside the compiler, and so do the crates only they use
	for build_time in ["serde_derive", "tokio-macros", "syn", "quote"] {
		assert_eq!(scope(build_time), Some("excluded".into()), "{build_time}");
	}
	// npm packages are bundled, so runtime
	assert_eq!(scope("vue"), Some("required".into()));
	// Every component has one of the two
	assert!(
		document["components"]
			.as_array()
			.unwrap()
			.iter()
			.all(|c| c["scope"] == "required" || c["scope"] == "excluded")
	);
}

#[tokio::test]
async fn npm_packages_come_from_the_frontend_folder() {
	let web = r#"{"packages":[
		{"name":"vue","version":"3.5.45","license":"MIT"},
		{"name":"@connectrpc/connect","version":"2.3.0","license":"Apache-2.0"},
		{"name":"no-license","version":"1.0.0"}
	]}"#;
	let (_, document) = fetch(Some(web), no_ffmpeg()).await.unwrap();

	let vue = by_name(&document, "vue").unwrap();
	assert_eq!(vue["type"], "library");
	assert_eq!(vue["purl"], "pkg:npm/vue@3.5.45");
	assert_eq!(vue["licenses"][0]["expression"], "MIT");
	let connect = by_name(&document, "@connectrpc/connect").unwrap();
	assert_eq!(connect["purl"], "pkg:npm/%40connectrpc/connect@2.3.0");
	let bare = by_name(&document, "no-license").unwrap();
	assert!(bare.get("licenses").is_none());
}

#[tokio::test]
async fn a_frontend_without_the_npm_list_is_fine() {
	let (parts, document) = fetch(None, no_ffmpeg()).await.unwrap();

	assert_eq!(parts.status, StatusCode::OK);
	assert!(
		components(&document, "library")
			.iter()
			.all(|c| !c["purl"].as_str().unwrap().starts_with("pkg:npm/"))
	);
}

#[tokio::test]
async fn ffmpeg_is_listed_when_it_runs() {
	let (_, without) = fetch(None, no_ffmpeg()).await.unwrap();
	assert_eq!(components(&without, "application"), Vec::<&Value>::new());

	// With the ffmpeg on PATH, when there is one (CI and the container have it)
	let (_, with) = fetch(None, Tools::default()).await.unwrap();
	let Some(ffmpeg) = by_name(&with, "ffmpeg") else {
		eprintln!("no ffmpeg on PATH; skipping the ffmpeg component check");
		return;
	};
	assert_eq!(ffmpeg["type"], "application");
	assert_ne!(ffmpeg["version"].as_str().unwrap(), "");
	// Licensed only when the build says GPL 3: the entry is either that or absent
	if let Some(licenses) = ffmpeg.get("licenses") {
		assert_eq!(licenses[0]["expression"], "GPL-3.0-or-later");
	}
}
