//! MuxMultiTool desktop binary: the Tauri app with the server embedded (see
//! the library's docs).

// A release build on Windows is a GUI program: no console window opens behind
// the app. Debug builds keep the console so the logs (stderr) are visible.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
	muxmultitool_desktop::init_logging();
	muxmultitool_desktop::run()
}
