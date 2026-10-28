//! Opaque ids for folders and files.
//!
//! An id is a [`Location`] in an opaque wrapping: which media root, and the
//! path under it, or an absolute path for a file the desktop app picked in a
//! native dialog. Clients store and return ids without looking inside, and
//! the encoding is not secret: the safety comes from [`MediaRoots`], which
//! checks every decoded location against the configured roots before it
//! touches the file system. A crafted id can name nothing a browse request
//! could not name too.
//!
//! The encoding is base64url without padding over a short text form, so an
//! id is URL-safe and stays the same across restarts as long as the roots
//! are configured in the same order.
//!
//! [`MediaRoots`]: super::MediaRoots

use std::fmt;
use std::path::{Component, Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// An opaque id as it crosses the wire.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileId(String);

impl FileId {
	/// Wraps an id received from a client. Nothing is checked until it is
	/// decoded.
	#[must_use]
	pub fn new(id: impl Into<String>) -> Self {
		Self(id.into())
	}

	/// The id as a string, for the wire.
	#[must_use]
	pub fn as_str(&self) -> &str {
		&self.0
	}

	/// Decodes the id.
	///
	/// # Errors
	/// When the id was not made by [`Location::id`]: not base64url, not the
	/// expected text form, or a relative path with a component other than a
	/// plain name (`..`, an empty one, or one with a separator).
	pub fn decode(&self) -> Result<Location, InvalidId> {
		let bytes = URL_SAFE_NO_PAD
			.decode(&self.0)
			.map_err(|_| InvalidId::Encoding)?;
		let text = String::from_utf8(bytes).map_err(|_| InvalidId::Encoding)?;
		Location::from_text(&text)
	}
}

impl fmt::Display for FileId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}

impl From<String> for FileId {
	fn from(id: String) -> Self {
		Self(id)
	}
}

impl From<&str> for FileId {
	fn from(id: &str) -> Self {
		Self(id.to_owned())
	}
}

/// An id that cannot be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidId {
	/// Not base64url, or not UTF-8 underneath.
	#[error("the id is not a valid id")]
	Encoding,
	/// Decoded, but not one of the known forms.
	#[error("the id has an unknown form")]
	Form,
	/// A path component that is not a plain name: `..`, `.`, empty, or an
	/// absolute path where a relative one is expected.
	#[error("the id names a path component that is not a plain name")]
	Component,
}

/// Where an id points. Only [`MediaRoots`](super::MediaRoots) resolves this
/// to a real path, after checking it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Location {
	/// A path under one of the media roots. The relative path is empty for
	/// the root itself and otherwise a chain of plain names, never `..`.
	InRoot {
		/// Index of the root in the configured list.
		root: usize,
		/// Path under the root.
		relative: PathBuf,
	},
	/// An absolute path from a native dialog. Only resolved in desktop mode.
	Absolute(PathBuf),
}

/// Separator used in the text form, whatever the OS uses.
const SEPARATOR: char = '/';

impl Location {
	/// The id for this location.
	///
	/// # Panics
	/// Never: every location has an id. Paths that are not UTF-8 are not
	/// representable, so [`MediaRoots`](super::MediaRoots) never builds a
	/// location for them.
	#[must_use]
	pub fn id(&self) -> FileId {
		FileId(URL_SAFE_NO_PAD.encode(self.to_text()))
	}

	/// The location one level down.
	#[must_use]
	pub fn child(&self, name: &str) -> Self {
		match self {
			Self::InRoot { root, relative } => Self::InRoot {
				root: *root,
				relative: relative.join(name),
			},
			Self::Absolute(path) => Self::Absolute(path.join(name)),
		}
	}

	/// The location one level up: `None` for a root and for an absolute
	/// path without a parent.
	#[must_use]
	pub fn parent(&self) -> Option<Self> {
		match self {
			Self::InRoot { root, relative } => relative.parent().map(|parent| Self::InRoot {
				root: *root,
				relative: parent.to_path_buf(),
			}),
			Self::Absolute(path) => path.parent().map(|p| Self::Absolute(p.to_path_buf())),
		}
	}

	/// `r<root>:<a/b/c>` or `a:<absolute path>`.
	fn to_text(&self) -> String {
		match self {
			Self::InRoot { root, relative } => {
				let names: Vec<&str> = relative
					.components()
					.filter_map(|c| match c {
						Component::Normal(name) => name.to_str(),
						_ => None,
					})
					.collect();
				format!("r{root}:{}", names.join(&SEPARATOR.to_string()))
			}
			Self::Absolute(path) => format!("a:{}", path.display()),
		}
	}

	fn from_text(text: &str) -> Result<Self, InvalidId> {
		if let Some(path) = text.strip_prefix("a:") {
			return Ok(Self::Absolute(PathBuf::from(path)));
		}
		let rest = text.strip_prefix('r').ok_or(InvalidId::Form)?;
		let (root, relative) = rest.split_once(':').ok_or(InvalidId::Form)?;
		let root = root.parse().map_err(|_| InvalidId::Form)?;
		let mut path = PathBuf::new();
		if !relative.is_empty() {
			for name in relative.split(SEPARATOR) {
				path.push(plain_name(name)?);
			}
		}
		Ok(Self::InRoot {
			root,
			relative: path,
		})
	}
}

/// `name` when it is exactly one normal path component on this OS.
fn plain_name(name: &str) -> Result<&str, InvalidId> {
	let mut components = Path::new(name).components();
	match (components.next(), components.next()) {
		(Some(Component::Normal(_)), None) => Ok(name),
		_ => Err(InvalidId::Component),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn in_root(root: usize, relative: &str) -> Location {
		Location::InRoot {
			root,
			relative: PathBuf::from(relative),
		}
	}

	#[test]
	fn ids_round_trip() {
		for location in [
			in_root(0, ""),
			in_root(3, "series/s01/e01.mkv"),
			in_root(1, "with space and ümlaut.mp4"),
			Location::Absolute(PathBuf::from("/home/me/clip.mp4")),
		] {
			let id = location.id();
			assert_eq!(id.decode().unwrap(), location, "{id}");
			assert_eq!(FileId::new(id.as_str()).decode().unwrap(), location);
		}
	}

	#[test]
	fn ids_are_url_safe_and_stable() {
		let id = in_root(0, "a/b.mp4").id();
		assert!(
			id.as_str()
				.chars()
				.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
			"{id}"
		);
		assert_eq!(id.as_str(), URL_SAFE_NO_PAD.encode("r0:a/b.mp4"));
		assert_eq!(in_root(0, "").id().as_str(), URL_SAFE_NO_PAD.encode("r0:"));
	}

	#[test]
	fn child_and_parent() {
		let root = in_root(2, "");
		let file = root.child("a").child("b.mp4");
		assert_eq!(file, in_root(2, "a/b.mp4"));
		assert_eq!(file.parent(), Some(in_root(2, "a")));
		assert_eq!(file.parent().unwrap().parent(), Some(root.clone()));
		assert_eq!(root.parent(), None);

		let abs = Location::Absolute(PathBuf::from("/x"));
		assert_eq!(
			abs.child("y").parent().map(|p| p.id()),
			Some(abs.id()),
			"absolute parents round-trip"
		);
	}

	#[test]
	fn crafted_ids_are_rejected() {
		let crafted = |text: &str| FileId::new(URL_SAFE_NO_PAD.encode(text)).decode();
		assert_eq!(crafted("r0:../etc/passwd"), Err(InvalidId::Component));
		assert_eq!(crafted("r0:a/../../etc"), Err(InvalidId::Component));
		assert_eq!(crafted("r0:./a"), Err(InvalidId::Component));
		assert_eq!(crafted("r0:/etc/passwd"), Err(InvalidId::Component));
		assert_eq!(crafted("r0:a//b"), Err(InvalidId::Component));
		assert_eq!(crafted("r0:a/"), Err(InvalidId::Component));
		assert_eq!(crafted("rx:a"), Err(InvalidId::Form));
		assert_eq!(crafted("r0"), Err(InvalidId::Form));
		assert_eq!(crafted("z:a"), Err(InvalidId::Form));
		assert_eq!(crafted(""), Err(InvalidId::Form));
		assert_eq!(
			FileId::new("not base64!").decode(),
			Err(InvalidId::Encoding)
		);
		assert_eq!(
			FileId::new(URL_SAFE_NO_PAD.encode([0xff, 0xfe])).decode(),
			Err(InvalidId::Encoding)
		);
	}

	#[cfg(unix)]
	#[test]
	fn backslashes_are_plain_characters_on_unix() {
		// On Windows the same id would be rejected as a separator
		assert_eq!(
			FileId::new(URL_SAFE_NO_PAD.encode("r0:a\\b")).decode(),
			Ok(in_root(0, "a\\b"))
		);
	}
}
