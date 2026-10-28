//! The full path from an HTTP request to the system service and the frontend.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use axum::body::Body;
use buffa::Message;
use connectrpc::CodecFormat;
use connectrpc::client::{ClientConfig, HttpClient};
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_proto::connect::muxmultitool::system::v1::SystemServiceClient;
use muxmultitool_proto::proto::muxmultitool::system::v1::{
	GetHealthRequest, GetHealthResponse, GetVersionRequest, HealthStatus,
};
use muxmultitool_server::{Config, Server, VERSION, app};
use tower::ServiceExt;

const HEALTH: &str = "/rpc/muxmultitool.system.v1.SystemService/GetHealth";
const VERSION_PATH: &str = "/rpc/muxmultitool.system.v1.SystemService/GetVersion";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A frontend folder with an `index.html` and one asset, like `vite build` makes.
fn frontend_dir() -> Result<tempfile::TempDir> {
	let dir = tempfile::tempdir()?;
	std::fs::write(
		dir.path().join("index.html"),
		"<!doctype html><title>mmt</title>",
	)?;
	std::fs::create_dir(dir.path().join("assets"))?;
	std::fs::write(dir.path().join("assets/app.js"), "console.log('mmt')")?;
	Ok(dir)
}

fn config(frontend_dir: &Path) -> Config {
	Config {
		address: IpAddr::V4(Ipv4Addr::LOCALHOST),
		port: 0,
		frontend_dir: frontend_dir.to_path_buf(),
		..Config::default()
	}
}

/// Sends `request` to the router in process, without a socket.
async fn send(request: Request<Body>) -> Result<(http::response::Parts, Vec<u8>)> {
	let dir = frontend_dir()?;
	let response = app(&config(dir.path())).oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes().to_vec();
	Ok((parts, body))
}

#[tokio::test]
async fn health_answers_a_plain_json_post() {
	let request = Request::post(HEALTH)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from("{}"))
		.unwrap();
	let (parts, body) = send(request).await.unwrap();

	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(parts.headers[header::CONTENT_TYPE], "application/json");
	let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
	assert_eq!(json, serde_json::json!({ "status": "HEALTH_STATUS_OK" }));
}

#[tokio::test]
async fn health_answers_a_connect_get() {
	// What the browser sends for a `NO_SIDE_EFFECTS` method
	let request = Request::get(format!("{HEALTH}?connect=v1&encoding=json&message=%7B%7D"))
		.body(Body::empty())
		.unwrap();
	let (parts, body) = send(request).await.unwrap();

	assert_eq!(parts.status, StatusCode::OK);
	let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
	assert_eq!(json["status"], "HEALTH_STATUS_OK");
}

#[tokio::test]
async fn health_answers_binary_protobuf() {
	// What the release frontend sends
	let request = Request::post(HEALTH)
		.header(header::CONTENT_TYPE, "application/proto")
		.body(Body::from(GetHealthRequest::default().encode_to_vec()))
		.unwrap();
	let (parts, body) = send(request).await.unwrap();

	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(parts.headers[header::CONTENT_TYPE], "application/proto");
	let response = GetHealthResponse::decode_from_slice(&body).unwrap();
	assert_eq!(
		response.status.to_i32(),
		HealthStatus::HEALTH_STATUS_OK as i32
	);
}

#[tokio::test]
async fn version_reports_the_crate_version() {
	let request = Request::post(VERSION_PATH)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from("{}"))
		.unwrap();
	let (parts, body) = send(request).await.unwrap();

	assert_eq!(parts.status, StatusCode::OK);
	let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
	assert_eq!(json["version"], VERSION);
	assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn unknown_rpc_is_a_connect_error_not_the_frontend() {
	let request = Request::post("/rpc/muxmultitool.system.v1.SystemService/Nope")
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from("{}"))
		.unwrap();
	let (parts, body) = send(request).await.unwrap();

	assert_eq!(parts.status, StatusCode::NOT_FOUND);
	let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
	assert_eq!(json["code"], "unimplemented");
}

#[tokio::test]
async fn frontend_is_served_at_the_root_with_history_fallback() {
	for path in ["/", "/index.html", "/jobs/42"] {
		let (parts, body) = send(Request::get(path).body(Body::empty()).unwrap())
			.await
			.unwrap();
		assert_eq!(parts.status, StatusCode::OK, "{path}");
		assert!(
			parts.headers[header::CONTENT_TYPE]
				.to_str()
				.unwrap()
				.starts_with("text/html"),
			"{path}"
		);
		assert_eq!(body, b"<!doctype html><title>mmt</title>", "{path}");
	}

	let (parts, body) = send(Request::get("/assets/app.js").body(Body::empty()).unwrap())
		.await
		.unwrap();
	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(body, b"console.log('mmt')");
}

#[tokio::test]
async fn generated_client_reaches_the_server_over_a_socket() {
	let dir = frontend_dir().unwrap();
	let server = Server::bind(&config(dir.path())).await.unwrap();
	let addr = server.local_addr().unwrap();
	let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
	let running = tokio::spawn(server.run(async {
		stopped.await.ok();
	}));

	let base: http::Uri = format!("http://{addr}/rpc").parse().unwrap();
	for format in [CodecFormat::Json, CodecFormat::Proto] {
		let config = ClientConfig::new(base.clone()).with_codec_format(format);
		let client = SystemServiceClient::new(HttpClient::plaintext(), config);

		let health = client
			.get_health(GetHealthRequest::default())
			.await
			.unwrap()
			.into_owned();
		assert_eq!(
			health.status.to_i32(),
			HealthStatus::HEALTH_STATUS_OK as i32,
			"{format:?}"
		);

		let version = client
			.get_version(GetVersionRequest::default())
			.await
			.unwrap()
			.into_owned();
		assert_eq!(version.version, VERSION, "{format:?}");
	}

	// The stop signal ends `run` cleanly
	stop.send(()).unwrap();
	running.await.unwrap().unwrap();
}

const CAPABILITIES: &str = "/rpc/muxmultitool.system.v1.SystemService/GetCapabilities";

async fn capabilities_json(config: Config) -> Result<serde_json::Value> {
	let request = Request::post(CAPABILITIES)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from("{}"))?;
	let response = app(&config).oneshot(request).await?;
	assert_eq!(response.status(), StatusCode::OK);
	let body = response.into_body().collect().await?.to_bytes();
	Ok(serde_json::from_slice(&body)?)
}

#[tokio::test]
async fn capabilities_describe_the_installed_ffmpeg() {
	let dir = frontend_dir().unwrap();
	let json = capabilities_json(config(dir.path())).await.unwrap();

	assert_eq!(json["status"], "CAPABILITIES_STATUS_AVAILABLE");
	let caps = &json["capabilities"];
	assert_ne!(caps["version"].as_str().unwrap(), "");
	assert!(
		caps["filters"]
			.as_array()
			.unwrap()
			.iter()
			.any(|f| f["name"] == "scale")
	);
}

#[tokio::test]
async fn capabilities_report_a_missing_ffmpeg_as_a_state() {
	let dir = frontend_dir().unwrap();
	let mut config = config(dir.path());
	config.tools.ffmpeg = "/nonexistent/ffmpeg".into();
	let json = capabilities_json(config).await.unwrap();

	assert_eq!(json["status"], "CAPABILITIES_STATUS_UNAVAILABLE");
	assert!(
		json["error"]
			.as_str()
			.unwrap()
			.contains("could not start ffmpeg")
	);
	assert!(json.get("capabilities").is_none());
}
