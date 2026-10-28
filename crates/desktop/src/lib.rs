//! MuxMultiTool desktop app.
//!
//! The same axum server as the container runs inside the Tauri process
//! (module [`backend`]), bound to `127.0.0.1` on a random port, and the window
//! loads the UI from it: page and API share one origin, exactly as in the
//! container. There is no separate backend process; closing the window stops
//! the server, which kills every running ffmpeg job.
//!
//! Only two things in the frontend are Tauri-specific, and both are IPC calls
//! that the capability file (`capabilities/main.json`) allows for the server's
//! origin alone: the native file dialogs behind `pickFiles()` (dialog plugin)
//! and opening external links in the system browser (opener plugin).
//!
//! Windows (`WebView2`) and macOS (`WKWebView`) use Tauri's wry runtime (module
//! `app`). The Linux shell, Tauri's CEF runtime, is a separate task: on Linux
//! [`run`] only says so, which keeps the workspace checks free of GTK and
//! `WebKit` dependencies.

use std::process::ExitCode;

pub mod backend;

#[cfg(any(windows, target_os = "macos"))]
mod app;

/// Logs to stderr, filtered by `RUST_LOG` (default `info`), like the server binary.
pub fn init_logging() {
	use tracing_subscriber::EnvFilter;

	tracing_subscriber::fmt()
		.with_env_filter(
			EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
		)
		.with_writer(std::io::stderr)
		.init();
}

/// Runs the desktop app until its window is closed.
#[cfg(any(windows, target_os = "macos"))]
#[must_use]
pub fn run() -> ExitCode {
	app::run()
}

/// The Linux desktop shell (Tauri's CEF runtime) is a separate task; this
/// binary only says so.
#[cfg(not(any(windows, target_os = "macos")))]
#[must_use]
pub fn run() -> ExitCode {
	tracing::error!(
		"the desktop app is built for Windows and macOS; the Linux shell (Tauri CEF runtime) is not implemented yet. Run the server (muxmultitool-server) and open it in a browser instead"
	);
	ExitCode::FAILURE
}
