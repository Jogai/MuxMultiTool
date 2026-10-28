//! Runs tauri-build for the desktop targets. It reads `tauri.conf.json` and
//! `capabilities/`, copies the ffmpeg and ffprobe sidecars (`binaries/`, see
//! `scripts/fetch-sidecars.sh`) next to the binary and, for Windows, compiles
//! the icon and version resource.
//!
//! On Linux this happens only with the `cef` feature (the CEF shell, see
//! `Cargo.toml`): tauri-build then also adds `$ORIGIN` to the binary's rpath,
//! which is where `libcef.so` sits in a bundle. Without the feature the Linux
//! binary is a stub and needs none of this.

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
	let cef = std::env::var_os("CARGO_FEATURE_CEF").is_some();
	if matches!(target_os.as_str(), "windows" | "macos") || (target_os == "linux" && cef) {
		tauri_build::build();
	}
}
