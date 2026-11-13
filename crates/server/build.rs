//! Records what goes into the binary, for the software bill of materials
//! the server serves at `GET /sbom.cdx.json` (module `sbom`).
//!
//! `cargo metadata` (through the `CARGO` cargo is running as) resolves the
//! workspace for the build target, and the script walks the `resolve` graph
//! from this package along the normal dependencies only (no `dev`, no
//! `build`), for the target only (`build/crate_graph.rs`). The crates it
//! finds, with their version, SPDX license expression, repository and
//! whether they are build time only (procedural macros and what only they
//! use), go to `sbom-crates.json` in `OUT_DIR`, together with the build time
//! and a serial number derived from those inputs, and `sbom.rs` embeds the
//! file with `include_str!`.
//!
//! The workspace crates themselves are left out: they are the application,
//! which the SBOM names as its root component. The desktop crate embeds the
//! server, so its binary serves the same list; the Tauri crates around it are
//! not in it.
//!
//! `cargo metadata` reads every manifest in the lock file, dev-dependencies
//! included, so it runs `--offline` first (nothing is fetched during a build)
//! and again with the network when a crate is missing from the local cache
//! (a release build never downloads dev-dependencies). An air-gapped build
//! runs `cargo fetch` first.

#[path = "build/crate_graph.rs"]
mod crate_graph;

use std::env;
use std::error::Error;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() -> Result<()> {
	println!("cargo:rerun-if-changed=build.rs");
	println!("cargo:rerun-if-changed=build/crate_graph.rs");
	let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
	let out_dir = PathBuf::from(env::var("OUT_DIR")?);
	let target = env::var("TARGET")?;

	let metadata = cargo_metadata(&manifest_dir, &target)?;
	rerun_on_manifest_changes(&metadata)?;
	let root = crate_graph::package_id(&metadata, &manifest_dir.join("Cargo.toml"))?;
	let crates = crate_graph::dependency_crates(&metadata, root)?;

	let timestamp = build_timestamp();
	let serial = serial_number(&crates, &timestamp, &target);
	let document = json!({
		"serialNumber": serial,
		"timestamp": timestamp,
		"target": target,
		"crates": crates,
	});
	fs::write(
		out_dir.join("sbom-crates.json"),
		serde_json::to_string(&document)?,
	)?;
	Ok(())
}

/// `cargo metadata` for the workspace, resolved for `target`.
fn cargo_metadata(manifest_dir: &Path, target: &str) -> Result<Value> {
	let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
	let run = |offline: bool| -> Result<Vec<u8>> {
		// The project rule routes process starts through `core::ffmpeg::Runner`,
		// which runs ffmpeg at run time; a build script starting cargo has no
		// such runner.
		#[allow(clippy::disallowed_methods)]
		let mut command = Command::new(&cargo);
		command
			.current_dir(manifest_dir)
			.args(["metadata", "--format-version", "1", "--locked"])
			.args(["--filter-platform", target]);
		if offline {
			command.arg("--offline");
		}
		let output = command.output()?;
		if !output.status.success() {
			let stderr = String::from_utf8_lossy(&output.stderr);
			return Err(format!(
				"cargo metadata failed ({}): {}",
				output.status,
				stderr.trim()
			)
			.into());
		}
		Ok(output.stdout)
	};
	let stdout = match run(true) {
		Ok(stdout) => stdout,
		Err(error) => {
			println!("cargo:warning=sbom: {error}; retrying with the network");
			run(false)?
		}
	};
	Ok(serde_json::from_slice(&stdout)?)
}

/// A changed lock file or manifest changes the dependency list.
fn rerun_on_manifest_changes(metadata: &Value) -> Result<()> {
	let root = metadata["workspace_root"]
		.as_str()
		.ok_or("cargo metadata has no workspace_root")?;
	println!(
		"cargo:rerun-if-changed={}",
		Path::new(root).join("Cargo.lock").display()
	);
	println!(
		"cargo:rerun-if-changed={}",
		Path::new(root).join("Cargo.toml").display()
	);
	for package in crate_graph::packages(metadata)?.values() {
		if crate_graph::is_workspace_member(metadata, package)
			&& let Some(manifest) = package["manifest_path"].as_str()
		{
			println!("cargo:rerun-if-changed={manifest}");
		}
	}
	Ok(())
}

/// Now, as RFC 3339 in UTC; `SOURCE_DATE_EPOCH` when set, for reproducible
/// builds.
fn build_timestamp() -> String {
	println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
	let seconds = env::var("SOURCE_DATE_EPOCH")
		.ok()
		.and_then(|value| value.parse::<i64>().ok())
		.or_else(|| {
			SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.ok()
				.and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
		})
		.unwrap_or_default();
	let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
	let rest = seconds.rem_euclid(86_400);
	format!(
		"{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
		rest / 3600,
		rest % 3600 / 60,
		rest % 60
	)
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
	let z = days + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z.rem_euclid(146_097);
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let day = doy - (153 * mp + 2) / 5 + 1;
	let month = if mp < 10 { mp + 3 } else { mp - 9 };
	let year = yoe + era * 400 + i64::from(month <= 2);
	// Both are in range by construction: a day is 1..=31, a month 1..=12.
	(
		year,
		u32::try_from(month).unwrap_or(1),
		u32::try_from(day).unwrap_or(1),
	)
}

/// A `urn:uuid:` serial number derived from the inputs, so the same build
/// gets the same number. Two independent hashes of the same input make the
/// 128 bits; the version (4) and variant bits are set so it is a well-formed
/// UUID.
fn serial_number(crates: &[Value], timestamp: &str, target: &str) -> String {
	let input = serde_json::to_string(crates).unwrap_or_default();
	let mut high = std::hash::DefaultHasher::new();
	let mut low = std::hash::DefaultHasher::new();
	(&input, timestamp, target).hash(&mut high);
	(target, timestamp, &input, "low").hash(&mut low);
	let high = (high.finish() & 0xffff_ffff_ffff_0fff) | 0x0000_0000_0000_4000;
	let low = (low.finish() & 0x3fff_ffff_ffff_ffff) | 0x8000_0000_0000_0000;
	format!(
		"urn:uuid:{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
		high >> 32,
		(high >> 16) & 0xffff,
		high & 0xffff,
		low >> 48,
		low & 0xffff_ffff_ffff
	)
}
