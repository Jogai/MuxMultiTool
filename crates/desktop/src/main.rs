//! MuxMultiTool desktop binary: the Tauri app with the server embedded (see
//! the library's docs).

// A release build on Windows is a GUI program: no console window opens behind
// the app. Debug builds keep the console so the logs (stderr) are visible.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

use std::process::ExitCode;

// On Linux the CEF runtime starts this same executable again for Chromium's
// renderer, GPU and utility processes (`--type=renderer` and the like). The
// attribute makes those runs hand over to CEF before anything else: no
// logging, no server, no window.
#[cfg_attr(
	all(target_os = "linux", feature = "cef"),
	tauri_runtime_cef::cef_entry_point
)]
fn main() -> ExitCode {
	muxmultitool_desktop::init_logging();
	muxmultitool_desktop::run()
}
