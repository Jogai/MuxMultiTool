//! Where ffmpeg and ffprobe live.
//!
//! The container image puts them on `PATH`; the desktop app ships them as
//! Tauri sidecars next to its own binary. Both end up as a [`Tools`] value,
//! so nothing else in the backend knows the difference.

use std::env;
use std::fmt;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the ffmpeg location.
pub const FFMPEG_ENV: &str = "MMT_FFMPEG";
/// Environment variable that overrides the ffprobe location.
pub const FFPROBE_ENV: &str = "MMT_FFPROBE";

/// One of the two external programs the runner can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
	/// The ffmpeg transcoder.
	Ffmpeg,
	/// The ffprobe analyser.
	Ffprobe,
}

impl Tool {
	/// Program name without directory or extension.
	#[must_use]
	pub const fn name(self) -> &'static str {
		match self {
			Self::Ffmpeg => "ffmpeg",
			Self::Ffprobe => "ffprobe",
		}
	}
}

impl fmt::Display for Tool {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.name())
	}
}

/// Paths of the ffmpeg and ffprobe executables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
	/// The ffmpeg executable, as given to the OS when spawning.
	pub ffmpeg: PathBuf,
	/// The ffprobe executable, as given to the OS when spawning.
	pub ffprobe: PathBuf,
}

impl Default for Tools {
	/// Bare program names, which the OS resolves through `PATH`.
	fn default() -> Self {
		Self {
			ffmpeg: PathBuf::from(Tool::Ffmpeg.name()),
			ffprobe: PathBuf::from(Tool::Ffprobe.name()),
		}
	}
}

impl Tools {
	/// Both programs in one directory, with the platform's executable suffix.
	/// This is the desktop layout, where they are bundled as sidecars.
	#[must_use]
	pub fn in_dir(dir: &Path) -> Self {
		let exe = |tool: Tool| dir.join(format!("{}{}", tool.name(), env::consts::EXE_SUFFIX));
		Self {
			ffmpeg: exe(Tool::Ffmpeg),
			ffprobe: exe(Tool::Ffprobe),
		}
	}

	/// [`Tools::default`], with [`FFMPEG_ENV`] and [`FFPROBE_ENV`] overriding
	/// each path when set and non-empty.
	#[must_use]
	pub fn from_env() -> Self {
		let mut tools = Self::default();
		if let Some(path) = env::var_os(FFMPEG_ENV).filter(|p| !p.is_empty()) {
			tools.ffmpeg = PathBuf::from(path);
		}
		if let Some(path) = env::var_os(FFPROBE_ENV).filter(|p| !p.is_empty()) {
			tools.ffprobe = PathBuf::from(path);
		}
		tools
	}

	/// The configured path of `tool`.
	#[must_use]
	pub fn path(&self, tool: Tool) -> &Path {
		match tool {
			Tool::Ffmpeg => &self.ffmpeg,
			Tool::Ffprobe => &self.ffprobe,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_uses_bare_names() {
		let tools = Tools::default();
		assert_eq!(tools.path(Tool::Ffmpeg), Path::new("ffmpeg"));
		assert_eq!(tools.path(Tool::Ffprobe), Path::new("ffprobe"));
	}

	#[test]
	fn in_dir_adds_exe_suffix() {
		let tools = Tools::in_dir(Path::new("/opt/mmt"));
		let expected = format!("/opt/mmt/ffprobe{}", env::consts::EXE_SUFFIX);
		assert_eq!(tools.ffprobe, PathBuf::from(expected));
	}
}
