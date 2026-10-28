//! Runs tauri-build for the desktop targets. It reads `tauri.conf.json` and
//! `capabilities/`, copies the ffmpeg and ffprobe sidecars (`binaries/`, see
//! `scripts/fetch-sidecars.sh`) next to the binary and, for Windows, compiles
//! the icon and version resource.
//!
//! Linux gets none of this yet: its shell (Tauri's CEF runtime) is a separate
//! task, and the binary is a stub there (see `src/lib.rs`).

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
	if matches!(target_os.as_str(), "windows" | "macos") {
		tauri_build::build();
	}
}
