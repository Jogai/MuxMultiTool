//! The container health check: `muxmultitool-server health`.
//!
//! A `HEALTHCHECK` runs inside the container, and the image has no shell or
//! curl, so the server binary itself is the client. It calls the health RPC
//! (`SystemService/GetHealth`, the same method the frontend uses) as a plain
//! JSON POST and exits 0 when the answer is `HEALTH_STATUS_OK`. There is no
//! separate health endpoint.
//!
//! The request is written by hand on a TCP socket: one small HTTP/1.1
//! request and response need no client library in the release binary.

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The health method's path under [`RPC_PREFIX`](crate::RPC_PREFIX).
pub const HEALTH_PATH: &str = "/rpc/muxmultitool.system.v1.SystemService/GetHealth";

/// How long a probe waits for the connection and the full answer together.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// The status string a healthy server answers with (proto enum name).
const HEALTHY: &str = "HEALTH_STATUS_OK";

/// Why a probe failed. Logged by the binary; the exit code is what Docker reads.
#[derive(Debug)]
pub enum ProbeError {
	/// The connection or the exchange failed.
	Io(io::Error),
	/// Nothing complete came back within [`PROBE_TIMEOUT`].
	Timeout,
	/// The server answered, but not with `200`.
	Status(String),
	/// The server answered `200`, but not `HEALTH_STATUS_OK`.
	Unhealthy(String),
}

impl fmt::Display for ProbeError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Io(error) => write!(f, "cannot reach the server: {error}"),
			Self::Timeout => write!(f, "no answer within {}s", PROBE_TIMEOUT.as_secs()),
			Self::Status(line) => write!(f, "unexpected answer: {line}"),
			Self::Unhealthy(body) => write!(f, "server is not healthy: {body}"),
		}
	}
}

impl std::error::Error for ProbeError {}

impl From<io::Error> for ProbeError {
	fn from(error: io::Error) -> Self {
		Self::Io(error)
	}
}

/// Calls the health RPC on `addr` and succeeds when the server reports
/// `HEALTH_STATUS_OK`.
///
/// # Errors
/// See [`ProbeError`].
pub async fn probe(addr: SocketAddr) -> Result<(), ProbeError> {
	tokio::time::timeout(PROBE_TIMEOUT, exchange(addr))
		.await
		.map_err(|_elapsed| ProbeError::Timeout)?
}

async fn exchange(addr: SocketAddr) -> Result<(), ProbeError> {
	let mut stream = TcpStream::connect(addr).await?;
	let request = format!(
		"POST {HEALTH_PATH} HTTP/1.1\r\n\
		 Host: {addr}\r\n\
		 Content-Type: application/json\r\n\
		 Content-Length: 2\r\n\
		 Connection: close\r\n\
		 \r\n\
		 {{}}"
	);
	stream.write_all(request.as_bytes()).await?;

	// `Connection: close` makes the end of the stream the end of the response.
	// The client side is not half-closed after the request: hyper's default
	// (`http1_half_close` off) drops a connection whose client went away.
	let mut response = Vec::new();
	stream.read_to_end(&mut response).await?;
	check(&String::from_utf8_lossy(&response))
}

/// Judges a raw HTTP/1.1 response: `200` and the healthy status in the body.
fn check(response: &str) -> Result<(), ProbeError> {
	let status_line = response.lines().next().unwrap_or_default();
	let mut words = status_line.split(' ');
	let (version, code) = (words.next().unwrap_or_default(), words.next());
	if !version.starts_with("HTTP/1.") || code != Some("200") {
		return Err(ProbeError::Status(status_line.to_owned()));
	}
	// The body may be chunked (hex sizes between the JSON); the status name
	// is still a plain substring either way.
	let body = response
		.split_once("\r\n\r\n")
		.map_or("", |(_headers, body)| body);
	if body.contains(HEALTHY) {
		Ok(())
	} else {
		Err(ProbeError::Unhealthy(body.trim().to_owned()))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ok_with_content_length() {
		let response = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 28\r\n\r\n{\"status\":\"HEALTH_STATUS_OK\"}";
		assert!(check(response).is_ok());
	}

	#[test]
	fn ok_when_chunked() {
		let response = "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n1c\r\n{\"status\":\"HEALTH_STATUS_OK\"}\r\n0\r\n\r\n";
		assert!(check(response).is_ok());
	}

	#[test]
	fn other_status_fails() {
		let response = "HTTP/1.1 404 Not Found\r\n\r\n{\"code\":\"unimplemented\"}";
		assert!(
			matches!(check(response), Err(ProbeError::Status(line)) if line == "HTTP/1.1 404 Not Found")
		);
	}

	#[test]
	fn other_health_status_fails() {
		let response = "HTTP/1.1 200 OK\r\n\r\n{\"status\":\"HEALTH_STATUS_UNSPECIFIED\"}";
		assert!(matches!(check(response), Err(ProbeError::Unhealthy(_))));
	}

	#[test]
	fn garbage_fails() {
		assert!(matches!(check(""), Err(ProbeError::Status(_))));
		assert!(matches!(check("nope"), Err(ProbeError::Status(_))));
	}
}
