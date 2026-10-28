//! File helpers shared by the modules that keep files on disk.

use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use tempfile::TempDir;

/// The folder a manager keeps its per-item subfolders in: one the caller
/// chose, or one made under the system temporary folder with a random
/// name, so no other process can pick it in advance.
#[derive(Debug)]
pub(crate) enum ScratchRoot {
	/// A folder of the caller's own; the manager creates it with its first
	/// subfolder and removes it when dropped.
	Given(PathBuf),
	/// A folder made under the system temporary folder on first use, named
	/// `<prefix><random>`, and removed when dropped.
	Temp {
		/// What the folder's name starts with.
		prefix: &'static str,
		/// The folder, once made.
		dir: Mutex<Option<TempDir>>,
	},
}

impl ScratchRoot {
	/// A root under the system temporary folder, made on the first call to
	/// [`ScratchRoot::path`].
	pub(crate) fn temp(prefix: &'static str) -> Self {
		Self::Temp {
			prefix,
			dir: Mutex::new(None),
		}
	}

	/// The folder, making it when it is a temporary one that does not exist
	/// yet. A given folder is only created with its first subfolder.
	///
	/// # Errors
	/// When the temporary folder cannot be made.
	pub(crate) fn path(&self) -> io::Result<PathBuf> {
		match self {
			Self::Given(path) => Ok(path.clone()),
			Self::Temp { prefix, dir } => {
				let mut dir = dir.lock().unwrap_or_else(PoisonError::into_inner);
				if let Some(made) = dir.as_ref() {
					return Ok(made.path().to_path_buf());
				}
				let made = tempfile::Builder::new().prefix(prefix).tempdir()?;
				let path = made.path().to_path_buf();
				*dir = Some(made);
				Ok(path)
			}
		}
	}

	/// The folder when it may exist: a given one always, a temporary one
	/// once it was made.
	pub(crate) fn existing(&self) -> Option<PathBuf> {
		match self {
			Self::Given(path) => Some(path.clone()),
			Self::Temp { dir, .. } => dir
				.lock()
				.unwrap_or_else(PoisonError::into_inner)
				.as_ref()
				.map(|made| made.path().to_path_buf()),
		}
	}

	/// Removes the folder and everything in it, for a drop: a temporary
	/// folder is removed by its [`TempDir`] anyway, a given one here.
	pub(crate) fn remove_on_drop(&self) {
		if let Self::Given(path) = self {
			let _ = std::fs::remove_dir_all(path);
		}
	}
}

impl From<PathBuf> for ScratchRoot {
	fn from(path: PathBuf) -> Self {
		Self::Given(path)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_temporary_root_is_made_once_with_a_random_name_and_removed_on_drop() {
		let root = ScratchRoot::temp("muxmultitool-test-");
		assert_eq!(root.existing(), None);
		let first = root.path().unwrap();
		assert_eq!(root.path().unwrap(), first);
		assert_eq!(root.existing(), Some(first.clone()));
		assert!(first.is_dir());
		let name = first.file_name().unwrap().to_string_lossy().into_owned();
		assert!(name.starts_with("muxmultitool-test-"), "{name}");
		assert!(name.len() > "muxmultitool-test-".len(), "{name}");
		let other = ScratchRoot::temp("muxmultitool-test-");
		assert_ne!(other.path().unwrap(), first);
		drop(root);
		drop(other);
		assert!(!first.exists());
	}

	#[test]
	fn a_given_root_is_left_to_the_caller_until_dropped() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("root");
		let root = ScratchRoot::from(path.clone());
		assert_eq!(root.existing(), Some(path.clone()));
		assert_eq!(root.path().unwrap(), path);
		assert!(!path.exists(), "a given root is not created by path()");
		std::fs::create_dir_all(path.join("item")).unwrap();
		root.remove_on_drop();
		assert!(!path.exists());
	}
}
