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
//! Windows (`WebView2`) and macOS (`WKWebView`) use Tauri's wry runtime, Linux
//! uses Tauri's CEF (Chromium) runtime (module `cef`), all through the same
//! module `app`. The Linux shell is behind the `cef` cargo feature so that the
//! workspace checks need neither GTK 4 nor the CEF binary distribution; without
//! it the Linux binary is a stub that says so.

use std::process::ExitCode;

pub mod backend;

#[cfg(any(
	windows,
	target_os = "macos",
	all(target_os = "linux", feature = "cef")
))]
mod app;
#[cfg(all(target_os = "linux", feature = "cef"))]
mod cef;

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
#[cfg(any(
	windows,
	target_os = "macos",
	all(target_os = "linux", feature = "cef")
))]
#[must_use]
pub fn run() -> ExitCode {
	app::run()
}

/// A Linux build without the `cef` feature has no shell; this binary only
/// says so.
#[cfg(not(any(
	windows,
	target_os = "macos",
	all(target_os = "linux", feature = "cef")
)))]
#[must_use]
pub fn run() -> ExitCode {
	tracing::error!(
		"this desktop binary was built without a shell: on Linux build it with `--features cef` (Tauri's CEF runtime, see crates/desktop/Cargo.toml), or run the server (muxmultitool-server) and open it in a browser instead"
	);
	ExitCode::FAILURE
}
