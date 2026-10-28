//! What the server needs to know before it starts.
//!
//! The container fills this from flags and `MMT_*` environment variables (see
//! the binary); the desktop app builds it in code with `127.0.0.1` and a
//! random port. Both end up with the same [`Config`].

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use muxmultitool_core::ffmpeg::Tools;

/// Port the server listens on unless told otherwise. The Vite dev server
/// proxies to it.
pub const DEFAULT_PORT: u16 = 8080;

/// Where `vite build` writes the frontend, relative to the repository root.
pub const DEFAULT_FRONTEND_DIR: &str = "web/dist";

/// Settings of one server instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
	/// Address to bind. Loopback by default; the container sets `0.0.0.0`.
	pub address: IpAddr,
	/// Port to bind. `0` asks the OS for a free port, which the desktop app
	/// uses; read the result from [`Server::local_addr`](crate::Server::local_addr).
	pub port: u16,
	/// Folders the file browser may show and jobs may read from and write to.
	/// Empty until a folder is configured, so nothing is exposed by accident.
	pub media_roots: Vec<PathBuf>,
	/// Where ffmpeg and ffprobe are.
	pub tools: Tools,
	/// Folder with the built frontend (`index.html` and `assets/`).
	pub frontend_dir: PathBuf,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			address: IpAddr::V4(Ipv4Addr::LOCALHOST),
			port: DEFAULT_PORT,
			media_roots: Vec::new(),
			tools: Tools::default(),
			frontend_dir: PathBuf::from(DEFAULT_FRONTEND_DIR),
		}
	}
}

impl Config {
	/// The address and port together.
	#[must_use]
	pub fn socket_addr(&self) -> SocketAddr {
		SocketAddr::new(self.address, self.port)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_listens_on_loopback_only() {
		let config = Config::default();
		assert!(config.socket_addr().ip().is_loopback());
		assert_eq!(config.socket_addr().port(), DEFAULT_PORT);
	}
}
