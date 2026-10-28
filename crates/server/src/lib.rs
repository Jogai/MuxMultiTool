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
//! - `/media/…`: the plain HTTP media routes (the HLS preview, module
//!   `media`; thumbnail sprites, module `timeline`); `GET /files/{id}`:
//!   downloads (module `download`).
//! - everything else: the built frontend, with `index.html` for unknown paths
//!   so the router's history mode works on a page reload.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use connectrpc::Router as ConnectRouter;
use files::Files;
use jobs::Jobs;
use muxmultitool_core::ffmpeg::{CapabilityCache, Runner};
use muxmultitool_core::files::MediaRoots;
use muxmultitool_core::jobs::{Queue, QueueConfig};
use muxmultitool_core::preview::{PreviewConfig, PreviewManager};
use muxmultitool_core::settings::SettingsStore;
use muxmultitool_core::timeline::{TimelineConfig, TimelineManager};
use system::System;
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

mod config;
mod download;
mod files;
mod gif;
mod jobs;
mod media;
mod report;
mod system;
mod timeline;

pub use config::{Config, DEFAULT_FRONTEND_DIR, DEFAULT_JOB_CONCURRENCY, DEFAULT_PORT};
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
	/// What the installed ffmpeg can do; see [`CapabilityCache`].
	pub capabilities: CapabilityCache,
	/// Resolves file ids to paths, and only to paths under `media_roots`
	/// (plus native-dialog paths in desktop mode, see [`Config::desktop`]).
	pub files: MediaRoots,
	/// See [`Config::frontend_dir`].
	pub frontend_dir: std::path::PathBuf,
	/// The job queue. Jobs outlive the request that submitted them, but
	/// not the server: see [`Server::run`].
	pub queue: Queue,
	/// The HLS preview sessions, with their segments under the system
	/// temporary folder (`TMPDIR`).
	pub previews: PreviewManager,
	/// The timeline analyses (thumbnails, waveform peaks, subtitle cues),
	/// with their sprites under the system temporary folder.
	pub timelines: TimelineManager,
	/// The user settings (the theme), in [`Config::data_dir`] when set.
	pub settings: SettingsStore,
}

impl AppState {
	/// The state for `config`.
	#[must_use]
	pub fn new(config: &Config) -> Self {
		let runner = Runner::new(config.tools.clone());
		Self {
			queue: Queue::new(
				runner.clone(),
				QueueConfig {
					concurrency: config.job_concurrency,
				},
			),
			previews: PreviewManager::in_temp_dir(runner.clone(), PreviewConfig::default()),
			timelines: TimelineManager::in_temp_dir(runner.clone(), TimelineConfig::default()),
			runner,
			media_roots: config.media_roots.clone(),
			capabilities: CapabilityCache::new(),
			files: MediaRoots::new(config.media_roots.clone(), config.desktop),
			frontend_dir: config.frontend_dir.clone(),
			settings: SettingsStore::from_config(config.data_dir.as_deref()),
		}
	}
}

/// Builds the full router for `config`.
///
/// This is also what the integration tests drive directly, without a socket.
/// The queue in the router's state is never shut down this way; drop the
/// router to kill its jobs, or use [`Server`] for a clean stop.
pub fn app(config: &Config) -> Router {
	app_with_state(Arc::new(AppState::new(config)))
}

/// Builds the full router around an existing `state`.
fn app_with_state(state: Arc<AppState>) -> Router {
	let rpc = ConnectRouter::new()
		.add_service(Arc::new(System::new(Arc::clone(&state))))
		.add_service(Arc::new(Files::new(Arc::clone(&state))))
		.add_service(Arc::new(media::Media {
			state: Arc::clone(&state),
		}))
		.add_service(Arc::new(Jobs {
			state: Arc::clone(&state),
		}))
		.add_service(Arc::new(gif::Gif::new(Arc::clone(&state))));

	let index = state.frontend_dir.join("index.html");
	let frontend = ServeDir::new(&state.frontend_dir).fallback(ServeFile::new(index));

	Router::new()
		.nest_service(RPC_PREFIX, rpc.into_axum_service())
		.merge(media::routes())
		.merge(timeline::routes())
		.merge(download::routes())
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
	state: Arc<AppState>,
}

impl Server {
	/// Binds the configured address and builds the router.
	///
	/// # Errors
	/// Fails when the address cannot be bound, typically because the port is in use.
	pub async fn bind(config: &Config) -> io::Result<Self> {
		warn_about_missing_folders(config);
		let listener = TcpListener::bind(config.socket_addr()).await?;
		let state = Arc::new(AppState::new(config));
		tokio::spawn(detect_at_startup(Arc::clone(&state)));
		Ok(Self {
			listener,
			app: app_with_state(Arc::clone(&state)),
			state,
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

	/// Serves until `shutdown` completes, then stops the job queue (every
	/// ffmpeg process is killed and waited for, every watch stream ends),
	/// waits for in-flight requests to finish, and finally stops the preview
	/// sessions and the timeline analyses and removes their files.
	///
	/// The queue stops before the drain: a watch stream is an in-flight
	/// response that only ends when its watch does, so draining first would
	/// wait for the last client to hang up.
	///
	/// # Errors
	/// Fails when accepting connections fails.
	pub async fn run(self, shutdown: impl Future<Output = ()> + Send + 'static) -> io::Result<()> {
		let addr = self.local_addr()?;
		info!(%addr, "listening");
		let queue = self.state.queue.clone();
		axum::serve(self.listener, self.app)
			.with_graceful_shutdown(async move {
				shutdown.await;
				queue.shutdown().await;
			})
			.await?;
		self.state.previews.shutdown().await;
		self.state.timelines.shutdown().await;
		info!("stopped");
		Ok(())
	}
}

/// Detects the ffmpeg capabilities once at startup, so the first request is
/// answered from the cache and a broken ffmpeg shows in the log right away.
async fn detect_at_startup(state: Arc<AppState>) {
	match state.capabilities.get(&state.runner).await {
		Ok(found) => info!(
			version = %found.version,
			encoders = found.encoders.len(),
			filters = found.filters.len(),
			hwaccels = ?found.hwaccels,
			"ffmpeg detected"
		),
		Err(error) => warn!(%error, "ffmpeg is missing or broken; jobs will fail until it works"),
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
