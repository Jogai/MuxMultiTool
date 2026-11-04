//! File helpers shared by the modules that keep files on disk.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use tempfile::TempDir;

/// Writes `contents` to `path` so that readers see either the old file or
/// the whole new one, never a partial write: the bytes go to a temporary
/// file in the same folder (made with a random name, so two writers cannot
/// trip over each other's temp file), are flushed to disk and the file is
/// then renamed over `path`. Missing parent folders are created.
///
/// # Errors
/// Any error of making the folder, writing, syncing or renaming, as an
/// [`io::Error`].
pub async fn write_atomically(path: &Path, contents: Vec<u8>) -> io::Result<()> {
	let path = path.to_path_buf();
	tokio::task::spawn_blocking(move || write_atomically_blocking(&path, &contents))
		.await
		.map_err(io::Error::other)?
}

fn write_atomically_blocking(path: &Path, contents: &[u8]) -> io::Result<()> {
	let dir = match path.parent() {
		Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
		_ => PathBuf::from("."),
	};
	std::fs::create_dir_all(&dir)?;
	let name = path
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_default();
	let mut temp = tempfile::Builder::new()
		.prefix(&format!(".{name}."))
		.suffix(".tmp")
		.tempfile_in(&dir)?;
	temp.write_all(contents)?;
	temp.as_file().sync_all()?;
	temp.persist(path).map_err(|error| error.error)?;
	Ok(())
}

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

	#[tokio::test]
	async fn writes_the_file_and_leaves_no_temp_file_behind() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("nested/state.json");
		write_atomically(&path, b"first".to_vec()).await.unwrap();
		assert_eq!(std::fs::read(&path).unwrap(), b"first");
		write_atomically(&path, b"second".to_vec()).await.unwrap();
		assert_eq!(std::fs::read(&path).unwrap(), b"second");
		let names: Vec<String> = std::fs::read_dir(dir.path().join("nested"))
			.unwrap()
			.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
			.collect();
		assert_eq!(names, ["state.json"]);
	}

	#[tokio::test]
	async fn a_folder_in_the_way_is_an_error_and_keeps_the_old_file() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("state.json");
		write_atomically(&path, b"old".to_vec()).await.unwrap();
		let blocked = dir.path().join("blocked");
		std::fs::create_dir(&blocked).unwrap();
		let error = write_atomically(&blocked, b"new".to_vec())
			.await
			.unwrap_err();
		assert!(blocked.is_dir(), "{error}");
		assert_eq!(std::fs::read(&path).unwrap(), b"old");
	}

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
