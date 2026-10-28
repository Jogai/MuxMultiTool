//! The Tauri app for Windows and macOS: starts the backend, opens the window
//! on it and stops the backend when the event loop ends.

use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{App, AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder};
use tracing::{error, info};

use crate::backend::{self, Backend, Paths};

/// The label `capabilities/main.json` grants the IPC permissions to.
const WINDOW_LABEL: &str = "main";

/// Where `tauri.conf.json` (`bundle.resources`) puts the built frontend
/// inside the resource folder.
const FRONTEND_RESOURCE: &str = "web";

/// How long the exit waits for the backend to kill its ffmpeg processes and
/// finish in-flight requests.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// The running backend, kept in Tauri's managed state until the app exits.
struct BackendState(Mutex<Option<Backend>>);

/// Builds and runs the app. The error path is an app that could not start
/// (no window, no port): it is logged and the process exits with a failure.
pub fn run() -> ExitCode {
	let app = tauri::Builder::default()
		.runtime(tauri_runtime_wry::Wry::default())
		.plugin(tauri_plugin_dialog::init())
		.plugin(tauri_plugin_opener::init())
		.setup(setup)
		.build(tauri::generate_context!());
	let app = match app {
		Ok(app) => app,
		Err(error) => {
			error!(%error, "cannot start the desktop app");
			return ExitCode::FAILURE;
		}
	};
	app.run(|handle, event| {
		if let RunEvent::Exit = event {
			stop_backend(handle);
		}
	});
	ExitCode::SUCCESS
}

/// Starts the backend on Tauri's tokio runtime and opens the window on it.
fn setup(app: &mut App) -> Result<(), Box<dyn Error>> {
	let paths = paths(app)?;
	let config = backend::config(&paths);
	info!(
		ffmpeg = %config.tools.ffmpeg.display(),
		ffprobe = %config.tools.ffprobe.display(),
		frontend_dir = %config.frontend_dir.display(),
		data_dir = ?config.data_dir,
		"starting the backend"
	);
	let backend = tauri::async_runtime::block_on(Backend::start(&config))?;
	let url = Url::parse(&backend.url())?;
	app.manage(BackendState(Mutex::new(Some(backend))));

	WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(url))
		.title("MuxMultiTool")
		.inner_size(1280.0, 800.0)
		.min_inner_size(800.0, 560.0)
		.build()?;
	Ok(())
}

/// The app-data folder (created when missing), the sidecars next to the
/// executable and the bundled frontend.
fn paths(app: &App) -> Result<Paths, Box<dyn Error>> {
	let exe = env::current_exe()?;
	let sidecar_dir = exe
		.parent()
		.ok_or("the executable has no parent folder")?
		.to_path_buf();
	let data_dir = app.path().app_data_dir()?;
	fs::create_dir_all(&data_dir)?;
	let bundled = app.path().resource_dir()?.join(FRONTEND_RESOURCE);
	let dev_fallback = cfg!(debug_assertions)
		.then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/dist"));
	Ok(Paths {
		data_dir,
		sidecar_dir,
		frontend_dir: backend::frontend_dir(bundled, dev_fallback),
	})
}

/// Takes the backend out of the state and stops it, blocking the main thread
/// for at most [`STOP_TIMEOUT`].
fn stop_backend(handle: &AppHandle) {
	let backend = handle
		.state::<BackendState>()
		.0
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner)
		.take();
	if let Some(backend) = backend {
		tauri::async_runtime::block_on(backend.stop(STOP_TIMEOUT));
	}
}
