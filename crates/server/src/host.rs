//! The `Host` check of desktop mode.
//!
//! The desktop server listens on loopback with a random port, and only its
//! own window is meant to reach it. A page on any other origin cannot read
//! its answers (there are no CORS headers), but DNS rebinding gets around
//! that: a page served from `attacker.example`, whose name is re-pointed at
//! `127.0.0.1` once the page is loaded, makes same-origin requests to the
//! local server. The `Host` header gives it away: the browser puts the
//! page's host there, not the local address. So in desktop mode a request
//! is only answered when its `Host` (or the `:authority` of an HTTP/2
//! request) names the local server: `127.0.0.1`, `localhost` or `[::1]`,
//! with the bound port when one is given. Anything else, including a
//! request without a `Host`, is `421 Misdirected Request`.
//!
//! The container has no such check: the name it is reached under is
//! whatever it is deployed behind (a reverse proxy, a compose service name,
//! a LAN address), and it is not tied to a browser on the same machine.

use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Answers only requests whose `Host` names the local server on `port`.
pub(crate) async fn local_only(State(port): State<u16>, request: Request, next: Next) -> Response {
	let host = request
		.headers()
		.get(header::HOST)
		.and_then(|value| value.to_str().ok())
		.or_else(|| {
			request
				.uri()
				.authority()
				.map(axum::http::uri::Authority::as_str)
		});
	if host.is_some_and(|host| is_local(host, port)) {
		next.run(request).await
	} else {
		(
			StatusCode::MISDIRECTED_REQUEST,
			"this server only answers requests addressed to 127.0.0.1 or localhost",
		)
			.into_response()
	}
}

/// Whether `host` is a loopback name, with `port` when it names a port.
fn is_local(host: &str, port: u16) -> bool {
	let (name, given_port) = match host.strip_prefix('[') {
		Some(rest) => match rest.split_once(']') {
			Some((ip, "")) => (ip, None),
			Some((ip, rest)) => (ip, rest.strip_prefix(':')),
			None => return false,
		},
		None => match host.rsplit_once(':') {
			Some((name, port)) => (name, Some(port)),
			None => (host, None),
		},
	};
	let local = name == "127.0.0.1" || name == "::1" || name.eq_ignore_ascii_case("localhost");
	local && given_port.is_none_or(|given| given.parse() == Ok(port))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn loopback_names_with_the_bound_port_are_local() {
		for host in [
			"127.0.0.1:4321",
			"localhost:4321",
			"LocalHost:4321",
			"[::1]:4321",
			"127.0.0.1",
			"localhost",
			"[::1]",
		] {
			assert!(is_local(host, 4321), "{host}");
		}
	}

	#[test]
	fn other_names_and_ports_are_not() {
		for host in [
			"attacker.example:4321",
			"attacker.example",
			"127.0.0.1:4322",
			"localhost:x",
			"127.0.0.1.attacker.example:4321",
			"localhost.attacker.example",
			"[::1",
			"[::2]:4321",
			"",
		] {
			assert!(!is_local(host, 4321), "{host}");
		}
	}
}
