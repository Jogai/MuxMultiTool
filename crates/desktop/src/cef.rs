//! The CEF (Chromium) runtime for the Linux shell: Tauri's `tauri-runtime-cef`
//! with the Chromium settings the app needs (architecture.md, "Desktop
//! specifics").
//!
//! CEF runs this executable again for its renderer, GPU and utility
//! processes; `main.rs` hands those over to CEF before the app starts. The
//! profile (cookies, local storage, caches) lives in the runtime's default
//! cache path under the user's cache folder, named after the app identifier.

use tauri_runtime_cef::Cef;

/// The runtime for `tauri::Builder::runtime`.
pub fn runtime() -> Cef {
	Cef::default()
		// The preview auto-plays after a seek, without a user gesture on the
		// element. Chromium's default autoplay policy would mute or block that
		// (HLS playback spike, issue 3); the app is a local tool, so autoplay
		// is simply allowed.
		.command_line_arg("autoplay-policy", Some("no-user-gesture-required"))
}
