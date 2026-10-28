//! The embedded server: [`muxmultitool_server::Server`] on `127.0.0.1` with a
//! random port, started before the window opens and stopped when the app
//! exits.
//!
//! Nothing here is Tauri-specific, so the module compiles and is tested on
//! every platform; the Tauri app (module `app`) only fills in the [`Paths`]
//! and calls [`Backend::start`] and [`Backend::stop`] on its tokio runtime.

use std::env;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::ffmpeg::Tools;
use muxmultitool_core::ffmpeg::tools::{FFMPEG_ENV, FFPROBE_ENV};
use muxmultitool_server::{Config, Server};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// Environment variable that points at a built frontend during development,
/// when nothing is bundled yet. The server binary reads the same one.
pub const FRONTEND_DIR_ENV: &str = "MMT_FRONTEND_DIR";

/// Where the desktop app keeps and finds its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
	/// The platform's app-data folder for this app (`%APPDATA%\<identifier>`
	/// on Windows, `~/Library/Application Support/<identifier>` on macOS):
	/// [`Config::data_dir`], where `settings.json` keeps the theme.
	pub data_dir: PathBuf,
	/// The folder with the ffmpeg and ffprobe sidecars: next to the app's own
	/// executable, which is where Tauri puts `bundle.externalBin`.
	pub sidecar_dir: PathBuf,
	/// The built frontend (`index.html` and `assets/`), bundled as a resource.
	pub frontend_dir: PathBuf,
}

/// The server configuration for the desktop app: loopback only, a random
/// port, native-dialog paths accepted (`desktop`), the sidecars as tools and
/// the settings in the app-data folder. No media roots: the user picks files
/// with the native dialogs instead of browsing configured folders.
///
/// `MMT_FFMPEG` and `MMT_FFPROBE` override the sidecars, for development on
/// a machine that has ffmpeg installed but no sidecars fetched.
#[must_use]
pub fn config(paths: &Paths) -> Config {
	Config {
		address: IpAddr::V4(Ipv4Addr::LOCALHOST),
		port: 0,
		tools: tools(
			&paths.sidecar_dir,
			env_path(FFMPEG_ENV).as_deref(),
			env_path(FFPROBE_ENV).as_deref(),
		),
		frontend_dir: paths.frontend_dir.clone(),
		desktop: true,
		data_dir: Some(paths.data_dir.clone()),
		..Config::default()
	}
}

/// The sidecars in `sidecar_dir`, each replaced by its override when given.
fn tools(sidecar_dir: &Path, ffmpeg: Option<&Path>, ffprobe: Option<&Path>) -> Tools {
	let mut tools = Tools::in_dir(sidecar_dir);
	if let Some(path) = ffmpeg {
		tools.ffmpeg = path.to_path_buf();
	}
	if let Some(path) = ffprobe {
		tools.ffprobe = path.to_path_buf();
	}
	tools
}

/// The value of a path variable, when set and not empty.
fn env_path(name: &str) -> Option<PathBuf> {
	env::var_os(name)
		.filter(|value| !value.is_empty())
		.map(PathBuf::from)
}

/// The frontend folder to serve: the bundled one when it holds an
/// `index.html`, otherwise [`FRONTEND_DIR_ENV`], otherwise `dev_fallback`
/// (the repository's `web/dist` in a debug build) when given.
///
/// A plain `cargo run` has no bundle, so without the fallback the window
/// would show the server's 404 page.
#[must_use]
pub fn frontend_dir(bundled: PathBuf, dev_fallback: Option<PathBuf>) -> PathBuf {
	if bundled.join("index.html").is_file() {
		return bundled;
	}
	if let Some(dir) = env_path(FRONTEND_DIR_ENV) {
		return dir;
	}
	dev_fallback.unwrap_or(bundled)
}

/// The running server. Dropping it without [`Backend::stop`] leaves the server
/// task running until the runtime shuts down; the Tauri app stops it on exit.
#[derive(Debug)]
pub struct Backend {
	addr: SocketAddr,
	stop: Option<oneshot::Sender<()>>,
	task: Option<JoinHandle<io::Result<()>>>,
}

impl Backend {
	/// Binds `config` and serves it on the current tokio runtime, so this must
	/// be called from within one (the Tauri app uses `tauri::async_runtime`).
	///
	/// # Errors
	/// Fails when the loopback address cannot be bound.
	pub async fn start(config: &Config) -> io::Result<Self> {
		let server = Server::bind(config).await?;
		let addr = server.local_addr()?;
		let (stop, stopped) = oneshot::channel();
		let task = tokio::spawn(server.run(async move {
			// A dropped sender stops the server as well as a sent signal.
			let _ = stopped.await;
		}));
		info!(%addr, "backend started");
		Ok(Self {
			addr,
			stop: Some(stop),
			task: Some(task),
		})
	}

	/// The bound address, with the port the OS picked.
	#[must_use]
	pub fn addr(&self) -> SocketAddr {
		self.addr
	}

	/// What the window loads: `http://127.0.0.1:{port}/`.
	#[must_use]
	pub fn url(&self) -> String {
		format!("http://{}/", self.addr)
	}

	/// Stops the server: the job queue kills every ffmpeg process and waits
	/// for it, in-flight requests finish, the preview and timeline files are
	/// removed. Gives up after `timeout` so the app always exits; the
	/// processes die with it then, their temporary files may stay behind.
	pub async fn stop(mut self, timeout: Duration) {
		if let Some(stop) = self.stop.take() {
			let _ = stop.send(());
		}
		let Some(task) = self.task.take() else {
			return;
		};
		match tokio::time::timeout(timeout, task).await {
			Ok(Ok(Ok(()))) => info!("backend stopped"),
			Ok(Ok(Err(error))) => warn!(%error, "backend failed"),
			Ok(Err(error)) => warn!(%error, "backend task did not finish"),
			Err(_) => warn!(?timeout, "backend did not stop in time; exiting anyway"),
		}
	}
}

#[cfg(test)]
mod tests {
	use std::fs;

	use tokio::io::{AsyncReadExt, AsyncWriteExt};
	use tokio::net::TcpStream;

	use super::*;

	fn exe(dir: &Path, name: &str) -> PathBuf {
		dir.join(format!("{name}{}", env::consts::EXE_SUFFIX))
	}

	#[test]
	fn config_is_loopback_random_port_desktop_mode() {
		let paths = Paths {
			data_dir: PathBuf::from("/data"),
			sidecar_dir: PathBuf::from("/app"),
			frontend_dir: PathBuf::from("/app/web"),
		};
		let config = config(&paths);

		assert!(config.socket_addr().ip().is_loopback());
		assert_eq!(config.socket_addr().port(), 0);
		assert!(config.desktop);
		assert_eq!(config.media_roots, Vec::<PathBuf>::new());
		assert_eq!(config.data_dir.as_deref(), Some(Path::new("/data")));
		assert_eq!(config.frontend_dir, Path::new("/app/web"));
		assert_eq!(
			config.job_concurrency,
			muxmultitool_server::DEFAULT_JOB_CONCURRENCY
		);
	}

	#[test]
	fn tools_are_the_sidecars_unless_overridden() {
		let dir = Path::new("/app");
		let sidecars = tools(dir, None, None);
		assert_eq!(sidecars.ffmpeg, exe(dir, "ffmpeg"));
		assert_eq!(sidecars.ffprobe, exe(dir, "ffprobe"));

		let overridden = tools(dir, Some(Path::new("/usr/bin/ffmpeg")), None);
		assert_eq!(overridden.ffmpeg, Path::new("/usr/bin/ffmpeg"));
		assert_eq!(overridden.ffprobe, exe(dir, "ffprobe"));
	}

	#[test]
	fn frontend_dir_prefers_the_bundle() {
		let bundled = tempfile::tempdir().unwrap();
		fs::write(bundled.path().join("index.html"), "<!doctype html>").unwrap();
		let fallback = PathBuf::from("/repo/web/dist");

		assert_eq!(
			frontend_dir(bundled.path().to_path_buf(), Some(fallback.clone())),
			bundled.path()
		);

		let empty = tempfile::tempdir().unwrap();
		assert_eq!(
			frontend_dir(empty.path().to_path_buf(), Some(fallback.clone())),
			fallback
		);
		assert_eq!(frontend_dir(empty.path().to_path_buf(), None), empty.path());
	}

	/// One request to the health method, as the frontend's Connect client
	/// would send it (JSON), over a raw socket so the test needs no client.
	async fn get_health(addr: SocketAddr) -> String {
		let mut stream = TcpStream::connect(addr).await.unwrap();
		let request = "POST /rpc/muxmultitool.system.v1.SystemService/GetHealth HTTP/1.1\r\n\
			Host: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\
			Connection: close\r\n\r\n{}";
		stream.write_all(request.as_bytes()).await.unwrap();
		let mut response = String::new();
		stream.read_to_string(&mut response).await.unwrap();
		response
	}

	#[tokio::test]
	async fn serves_on_a_random_loopback_port_until_stopped() {
		let frontend = tempfile::tempdir().unwrap();
		fs::write(frontend.path().join("index.html"), "<!doctype html>").unwrap();
		let data = tempfile::tempdir().unwrap();
		let paths = Paths {
			data_dir: data.path().to_path_buf(),
			sidecar_dir: PathBuf::from("/nowhere"),
			frontend_dir: frontend.path().to_path_buf(),
		};

		let backend = Backend::start(&config(&paths)).await.unwrap();
		let addr = backend.addr();
		assert!(addr.ip().is_loopback());
		assert_ne!(addr.port(), 0);
		assert_eq!(backend.url(), format!("http://127.0.0.1:{}/", addr.port()));

		let response = get_health(addr).await;
		assert!(response.starts_with("HTTP/1.1 200"), "{response}");
		assert!(response.contains("HEALTH_STATUS_OK"), "{response}");

		backend.stop(Duration::from_secs(10)).await;
		assert!(
			TcpStream::connect(addr).await.is_err(),
			"the port must be closed after stop"
		);
	}
}
