//! MuxMultiTool server.
//!
//! One axum server for both deployments: the container runs it as the main
//! process (see the binary), the desktop app runs it on its own tokio runtime
//! and points the window at it. Either way the frontend calls its own origin.
//!
//! Routes:
//! - `/rpc/…`: the ConnectRPC services, as a Tower service mounted on the
//!   axum router. Connect decides the encoding per request: the frontend sends
//!   JSON in development and binary protobuf in release builds, and `curl`
//!   can always send JSON.
//! - `/media/…` and `/files/…`: the plain HTTP media routes, added by later
//!   features next to the RPC mount.
//! - everything else: the built frontend, with `index.html` for unknown paths
//!   so the router's history mode works on a page reload.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use connectrpc::Router as ConnectRouter;
use muxmultitool_core::ffmpeg::Runner;
use system::System;
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

mod config;
mod system;

pub use config::{Config, DEFAULT_FRONTEND_DIR, DEFAULT_PORT};
pub use system::VERSION;

/// Path prefix of the Connect router.
pub const RPC_PREFIX: &str = "/rpc";

/// State shared by every handler. Services get it as an [`Arc`].
#[derive(Debug)]
pub struct AppState {
	/// Starts ffmpeg and ffprobe at the configured paths.
	pub runner: Runner,
	/// See [`Config::media_roots`].
	pub media_roots: Vec<std::path::PathBuf>,
}

/// Builds the full router for `config`.
///
/// This is also what the integration tests drive directly, without a socket.
pub fn app(config: &Config) -> Router {
	let state = Arc::new(AppState {
		runner: Runner::new(config.tools.clone()),
		media_roots: config.media_roots.clone(),
	});

	let rpc = ConnectRouter::new().add_service(Arc::new(System));

	let index = config.frontend_dir.join("index.html");
	let frontend = ServeDir::new(&config.frontend_dir).fallback(ServeFile::new(index));

	Router::new()
		.nest_service(RPC_PREFIX, rpc.into_axum_service())
		.fallback_service(frontend)
		.layer(TraceLayer::new_for_http())
		.with_state(state)
}

/// A bound listener and the router that serves it.
///
/// Binding and serving are separate steps, so the desktop app can bind port
/// `0` and learn the port before it opens the window.
#[derive(Debug)]
pub struct Server {
	listener: TcpListener,
	app: Router,
}

impl Server {
	/// Binds the configured address and builds the router.
	///
	/// # Errors
	/// Fails when the address cannot be bound, typically because the port is in use.
	pub async fn bind(config: &Config) -> io::Result<Self> {
		warn_about_missing_folders(config);
		let listener = TcpListener::bind(config.socket_addr()).await?;
		Ok(Self {
			listener,
			app: app(config),
		})
	}

	/// The bound address, with the real port when the config asked for `0`.
	///
	/// # Errors
	/// Only if the OS cannot report the socket's address, which does not
	/// happen for a bound listener.
	pub fn local_addr(&self) -> io::Result<SocketAddr> {
		self.listener.local_addr()
	}

	/// Serves until `shutdown` completes, then waits for in-flight requests
	/// to finish.
	///
	/// # Errors
	/// Fails when accepting connections fails.
	pub async fn run(self, shutdown: impl Future<Output = ()> + Send + 'static) -> io::Result<()> {
		let addr = self.local_addr()?;
		info!(%addr, "listening");
		axum::serve(self.listener, self.app)
			.with_graceful_shutdown(shutdown)
			.await?;
		info!("stopped");
		Ok(())
	}
}

/// Completes on Ctrl-C or, on Unix, `SIGTERM`: what `docker stop` and a
/// terminal send.
pub async fn shutdown_signal() {
	let ctrl_c = async {
		if let Err(error) = tokio::signal::ctrl_c().await {
			warn!(%error, "cannot listen for ctrl-c");
			std::future::pending::<()>().await;
		}
	};

	#[cfg(unix)]
	let terminate = async {
		use tokio::signal::unix::{SignalKind, signal};
		match signal(SignalKind::terminate()) {
			Ok(mut stream) => {
				stream.recv().await;
			}
			Err(error) => {
				warn!(%error, "cannot listen for SIGTERM");
				std::future::pending::<()>().await;
			}
		}
	};
	#[cfg(not(unix))]
	let terminate = std::future::pending::<()>();

	tokio::select! {
		() = ctrl_c => info!("ctrl-c received, shutting down"),
		() = terminate => info!("stop signal received, shutting down"),
	}
}

/// A missing folder is not an error (the container may mount it later), but
/// it is the first thing to check when the page or the file browser is empty.
fn warn_about_missing_folders(config: &Config) {
	if !config.frontend_dir.join("index.html").is_file() {
		warn!(dir = %config.frontend_dir.display(), "no index.html in the frontend folder; / will answer 404");
	}
	for root in &config.media_roots {
		if !root.is_dir() {
			warn!(root = %root.display(), "media root is not a folder");
		}
	}
	if config.media_roots.is_empty() {
		info!("no media roots configured");
	}
}
