//! Detects the capabilities of the real ffmpeg from `PATH`, and of paths that
//! are missing or broken.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use muxmultitool_core::ffmpeg::capabilities::detect;
use muxmultitool_core::ffmpeg::{CapabilityCache, CapabilityError, RunError, Runner, Tools};

#[tokio::test]
async fn detects_the_installed_build() {
	let found = detect(&Runner::default()).await.unwrap();
	assert_ne!(found.version, "");
	assert!(found.has_filter("scale"));
	assert!(found.has_bitstream_filter("h264_mp4toannexb"));
	assert!(found.encoders.iter().any(|c| c.name == "aac"));
	assert!(found.formats.iter().any(|f| f.name == "mp4" && f.mux));
}

#[tokio::test]
async fn cache_returns_the_same_value_while_the_version_holds() {
	let runner = Runner::default();
	let cache = CapabilityCache::new();
	let first = cache.get(&runner).await.unwrap();
	let second = cache.get(&runner).await.unwrap();
	assert!(std::sync::Arc::ptr_eq(&first, &second));
}

#[tokio::test]
async fn cache_detects_again_when_the_version_changes() {
	let dir = tempfile::tempdir().unwrap();
	let fake = dir.path().join("ffmpeg");
	let script = |version: &str| {
		format!(
			"#!/bin/sh\ncase \"$1$2\" in\n\t*-version) echo 'ffmpeg version {version} Copyright';;\n\t*-hwaccels) echo 'Hardware acceleration methods:'; echo vaapi;;\n\t*) echo ' ------';;\nesac\n"
		)
	};
	let runner = Runner::new(Tools {
		ffmpeg: fake.clone(),
		..Tools::default()
	});
	let cache = CapabilityCache::new();

	write_executable(&fake, &script("1.0")).unwrap();
	let first = cache.get(&runner).await.unwrap();
	assert_eq!(first.version, "1.0");
	assert_eq!(first.hwaccels, ["vaapi"]);

	write_executable(&fake, &script("2.0")).unwrap();
	let second = cache.get(&runner).await.unwrap();
	assert_eq!(second.version, "2.0");
}

#[tokio::test]
async fn a_missing_ffmpeg_is_an_error_not_a_panic() {
	let runner = Runner::new(Tools {
		ffmpeg: "/nonexistent/ffmpeg".into(),
		..Tools::default()
	});
	let error = CapabilityCache::new().get(&runner).await.unwrap_err();
	assert!(matches!(
		error,
		CapabilityError::Run(RunError::Spawn { .. })
	));
}

#[tokio::test]
async fn an_ffmpeg_that_prints_nonsense_is_an_error() {
	let dir = tempfile::tempdir().unwrap();
	let fake = dir.path().join("ffmpeg");
	write_executable(&fake, "#!/bin/sh\necho hello\n").unwrap();
	let runner = Runner::new(Tools {
		ffmpeg: fake,
		..Tools::default()
	});
	let error = detect(&runner).await.unwrap_err();
	assert!(matches!(error, CapabilityError::UnknownVersion));
}

#[tokio::test]
async fn an_ffmpeg_that_fails_is_an_error() {
	let dir = tempfile::tempdir().unwrap();
	let fake = dir.path().join("ffmpeg");
	write_executable(&fake, "#!/bin/sh\necho 'broken install' >&2\nexit 3\n").unwrap();
	let runner = Runner::new(Tools {
		ffmpeg: fake,
		..Tools::default()
	});
	let error = detect(&runner).await.unwrap_err();
	assert!(matches!(
		error,
		CapabilityError::Run(RunError::Failed { .. })
	));
}

/// Writes a script and makes it executable, via a rename so a running copy
/// is never written to ("text file busy").
fn write_executable(path: &std::path::Path, content: &str) -> std::io::Result<()> {
	let tmp = path.with_extension("tmp");
	fs::write(&tmp, content)?;
	fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
	fs::rename(&tmp, path)
}
