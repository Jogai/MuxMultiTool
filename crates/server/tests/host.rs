//! The `Host` check: desktop mode answers only requests addressed to the
//! local server; the container answers whatever name it is reached under.

#![expect(
	clippy::unwrap_used,
	reason = "a failed unwrap fails the test, and the helpers are shared by tests"
)]

use std::net::{IpAddr, Ipv4Addr};

use axum::Router;
use axum::body::Body;
use http::{Request, StatusCode, header};
use muxmultitool_server::{Config, app};
use tower::ServiceExt;

const HEALTH: &str = "/rpc/muxmultitool.system.v1.SystemService/GetHealth";

fn router(desktop: bool) -> Router {
	app(&Config {
		address: IpAddr::V4(Ipv4Addr::LOCALHOST),
		port: 4321,
		desktop,
		..Config::default()
	})
}

async fn health(router: &Router, host: Option<&str>) -> StatusCode {
	let mut request = Request::post(HEALTH).header(header::CONTENT_TYPE, "application/json");
	if let Some(host) = host {
		request = request.header(header::HOST, host);
	}
	let request = request.body(Body::from("{}")).unwrap();
	router.clone().oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn desktop_mode_answers_the_local_server_only() {
	let router = router(true);
	for host in [
		"127.0.0.1:4321",
		"localhost:4321",
		"[::1]:4321",
		"127.0.0.1",
	] {
		assert_eq!(health(&router, Some(host)).await, StatusCode::OK, "{host}");
	}
	for host in [
		"attacker.example:4321",
		"127.0.0.1:4322",
		"attacker.example",
	] {
		assert_eq!(
			health(&router, Some(host)).await,
			StatusCode::MISDIRECTED_REQUEST,
			"{host}"
		);
	}
	assert_eq!(
		health(&router, None).await,
		StatusCode::MISDIRECTED_REQUEST,
		"no host"
	);

	// The frontend fallback is behind the check too
	let request = Request::get("/")
		.header(header::HOST, "attacker.example:4321")
		.body(Body::empty())
		.unwrap();
	let response = router.clone().oneshot(request).await.unwrap();
	assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn the_container_answers_any_host() {
	let router = router(false);
	for host in [Some("media.example"), Some("10.0.0.5:8080"), None] {
		assert_eq!(health(&router, host).await, StatusCode::OK, "{host:?}");
	}
}
