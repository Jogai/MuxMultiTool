//! Browsing the media folders and naming files by opaque ids.
//!
//! The container works on mounted folders, the *media roots*. Everything a
//! client sees is an [`Entry`] with an opaque [`FileId`]; the client never
//! sends a path, except the desktop app, which passes the result of a native
//! dialog to [`MediaRoots::open_path`].
//!
//! Every id goes through the same check (see [`MediaRoots::path`]) before the file system is
//! touched. It canonicalizes the path (which follows symlinks) and checks
//! that the result is still under the canonical root, so neither `..` nor a
//! symlink can reach outside. Listings apply the same check to each entry,
//! so a symlink that points out of the root is simply not shown.

pub mod glob;
pub mod id;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use glob::{Pattern, PatternError};
pub use id::{FileId, InvalidId, Location};
use tracing::debug;

/// File extensions the app can open, lowercase without the dot. Anything
/// else is left out of listings: the app could do nothing with it anyway.
pub const MEDIA_EXTENSIONS: &[&str] = &[
	// video containers
	"3gp", "avi", "flv", "m2ts", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "mts", "ogv", "ts",
	"vob", "webm", "wmv", // audio
	"aac", "ac3", "dts", "eac3", "flac", "m4a", "mka", "mp3", "ogg", "opus", "wav", "wma",
	// subtitles, for muxing into a file
	"ass", "srt", "ssa", "sub", "sup", "vtt", // images the app converts
	"gif",
];

/// Whether a file name has one of the [`MEDIA_EXTENSIONS`].
#[must_use]
pub fn is_media_name(name: &str) -> bool {
	Path::new(name)
		.extension()
		.and_then(|ext| ext.to_str())
		.is_some_and(|ext| MEDIA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// What went wrong with a files request.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// The id is not one the server made.
	#[error("invalid id: {0}")]
	InvalidId(#[from] InvalidId),
	/// The id names a root that is not configured or a path that does not exist.
	#[error("not found")]
	NotFound,
	/// The path resolves outside the media roots, through a symlink.
	#[error("the path is outside the media roots")]
	OutsideRoots,
	/// A path was sent by a client, which only desktop mode accepts.
	#[error("paths from the client are only accepted in desktop mode")]
	PathsNotAccepted,
	/// A relative path was sent where an absolute one is required.
	#[error("the path is not absolute")]
	NotAbsolute,
	/// A folder was expected.
	#[error("not a folder")]
	NotAFolder,
	/// A wildcard pattern could not be parsed.
	#[error("invalid pattern: {0}")]
	Pattern(#[from] PatternError),
	/// No pattern was given.
	#[error("at least one pattern is required")]
	NoPatterns,
	/// The file system refused.
	#[error("cannot read {}: {source}", path.display())]
	Io {
		/// What was being read.
		path: PathBuf,
		/// The OS error.
		#[source]
		source: io::Error,
	},
}

/// Folder or file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
	/// A folder, which [`MediaRoots::list`] and [`MediaRoots::find`] accept.
	Folder,
	/// A media file.
	File,
}

/// One folder or media file, as shown to the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
	/// The opaque id.
	pub id: FileId,
	/// File or folder name without its path. For a media root, the name of
	/// the mounted folder.
	pub name: String,
	/// Folder or file.
	pub kind: Kind,
	/// Size in bytes, `0` for folders.
	pub size: u64,
	/// Display path: relative to the media root, with `/` separators and
	/// empty for the root itself; the absolute path for a desktop entry.
	pub path: String,
}

/// A folder's contents, with the breadcrumbs that lead to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listing {
	/// The listed folder, `None` for the roots listing.
	pub folder: Option<Entry>,
	/// From the root down to the parent. Empty for a root.
	pub ancestors: Vec<Entry>,
	/// Subfolders first, then media files, each sorted by name.
	pub entries: Vec<Entry>,
}

/// The folders the app may touch, and whether client paths are trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRoots {
	roots: Vec<PathBuf>,
	accept_paths: bool,
}

/// A resolved location: the real path and what is there.
#[derive(Debug)]
struct Resolved {
	location: Location,
	path: PathBuf,
	metadata: fs::Metadata,
}

impl MediaRoots {
	/// `roots` are the mounted folders, in the order that the ids encode.
	/// `accept_paths` is the desktop mode: absolute paths from
	/// [`open_path`](Self::open_path) are then trusted.
	#[must_use]
	pub fn new(roots: Vec<PathBuf>, accept_paths: bool) -> Self {
		Self {
			roots,
			accept_paths,
		}
	}

	/// The configured roots.
	#[must_use]
	pub fn roots(&self) -> &[PathBuf] {
		&self.roots
	}

	/// Whether absolute paths from a client are accepted (desktop mode).
	#[must_use]
	pub fn accepts_paths(&self) -> bool {
		self.accept_paths
	}

	/// Lists `folder`, or the roots when `None`.
	///
	/// # Errors
	/// [`Error::NotAFolder`] when the id names a file, and whatever
	/// [`path`](Self::path) returns.
	pub fn list(&self, folder: Option<&FileId>) -> Result<Listing, Error> {
		let Some(folder) = folder else {
			return Ok(Listing {
				entries: self.root_entries(),
				..Listing::default()
			});
		};
		let resolved = self.resolve_id(folder)?;
		if !resolved.metadata.is_dir() {
			return Err(Error::NotAFolder);
		}
		let entries = self.read_folder(&resolved)?;
		Ok(Listing {
			ancestors: self.ancestors(&resolved.location),
			folder: Some(self.entry(&resolved)),
			entries,
		})
	}

	/// The media files in `folder` whose names match any of `patterns`,
	/// sorted by path. With `recursive`, subfolders are searched too, except
	/// through symlinks, so a link back up cannot loop.
	///
	/// # Errors
	/// [`Error::NoPatterns`] or [`Error::Pattern`] for bad patterns,
	/// [`Error::NotAFolder`] when the id names a file, and whatever
	/// [`path`](Self::path) returns.
	pub fn find(
		&self,
		folder: &FileId,
		patterns: &[String],
		recursive: bool,
	) -> Result<Vec<Entry>, Error> {
		if patterns.is_empty() {
			return Err(Error::NoPatterns);
		}
		let patterns = patterns
			.iter()
			.map(|p| Pattern::parse(p))
			.collect::<Result<Vec<_>, _>>()?;
		let resolved = self.resolve_id(folder)?;
		if !resolved.metadata.is_dir() {
			return Err(Error::NotAFolder);
		}
		let mut found = Vec::new();
		let mut folders = vec![resolved];
		while let Some(folder) = folders.pop() {
			for entry in self.read_folder(&folder)? {
				match entry.kind {
					Kind::File if Pattern::any_matches(&patterns, &entry.name) => found.push(entry),
					Kind::Folder if recursive => {
						let child = folder.path.join(&entry.name);
						let is_symlink =
							fs::symlink_metadata(&child).is_ok_and(|m| m.file_type().is_symlink());
						if !is_symlink
							&& let Ok(sub) = self.resolve(&folder.location.child(&entry.name))
						{
							folders.push(sub);
						}
					}
					_ => {}
				}
			}
		}
		found.sort_by(|a, b| a.path.cmp(&b.path));
		Ok(found)
	}

	/// The entry behind an id, for a client that stored it.
	///
	/// # Errors
	/// Whatever [`path`](Self::path) returns, and [`Error::NotFound`]
	/// when the id names something that is neither a folder nor a media file.
	pub fn get(&self, id: &FileId) -> Result<Entry, Error> {
		let resolved = self.resolve_id(id)?;
		if !resolved.metadata.is_dir() && !is_media_name(&resolved.path.to_string_lossy()) {
			return Err(Error::NotFound);
		}
		Ok(self.entry(&resolved))
	}

	/// The real path behind an id, for the parts of the app that run ffmpeg.
	///
	/// The path is canonical and inside one of the roots, or it is an
	/// absolute path in desktop mode.
	///
	/// # Errors
	/// [`Error::InvalidId`] for an id the server did not make,
	/// [`Error::NotFound`] when the root is not configured or the path does
	/// not exist, [`Error::OutsideRoots`] when a symlink leads outside the
	/// root, [`Error::PathsNotAccepted`] for an absolute path outside
	/// desktop mode.
	pub fn path(&self, id: &FileId) -> Result<PathBuf, Error> {
		Ok(self.resolve_id(id)?.path)
	}

	/// The entry for an absolute path from a native dialog: desktop mode only.
	///
	/// # Errors
	/// [`Error::PathsNotAccepted`] outside desktop mode, [`Error::NotAbsolute`]
	/// for a relative path, [`Error::NotFound`] when there is no folder or
	/// media file at the path.
	pub fn open_path(&self, path: &Path) -> Result<Entry, Error> {
		if !self.accept_paths {
			return Err(Error::PathsNotAccepted);
		}
		if !path.is_absolute() {
			return Err(Error::NotAbsolute);
		}
		self.get(&Location::Absolute(path.to_path_buf()).id())
	}

	fn resolve_id(&self, id: &FileId) -> Result<Resolved, Error> {
		self.resolve(&id.decode()?)
	}

	/// The containment check. Everything that reads the file system comes
	/// through here.
	fn resolve(&self, location: &Location) -> Result<Resolved, Error> {
		let path = match location {
			Location::InRoot { root, relative } => {
				let root = self.roots.get(*root).ok_or(Error::NotFound)?;
				let canonical_root = canonicalize(root)?;
				let path = canonicalize(&root.join(relative))?;
				if !path.starts_with(&canonical_root) {
					debug!(path = %path.display(), root = %canonical_root.display(), "outside the root");
					return Err(Error::OutsideRoots);
				}
				path
			}
			Location::Absolute(path) => {
				if !self.accept_paths {
					return Err(Error::PathsNotAccepted);
				}
				if !path.is_absolute() {
					return Err(Error::NotAbsolute);
				}
				canonicalize(path)?
			}
		};
		let metadata = fs::metadata(&path).map_err(|source| io_error(&path, source))?;
		Ok(Resolved {
			location: location.clone(),
			path,
			metadata,
		})
	}

	fn root_entries(&self) -> Vec<Entry> {
		(0..self.roots.len())
			.filter_map(|root| {
				let location = Location::InRoot {
					root,
					relative: PathBuf::new(),
				};
				match self.resolve(&location) {
					Ok(resolved) if resolved.metadata.is_dir() => Some(self.entry(&resolved)),
					_ => {
						debug!(root = %self.roots[root].display(), "media root is not a folder, left out");
						None
					}
				}
			})
			.collect()
	}

	/// The folders above `location`, root first.
	fn ancestors(&self, location: &Location) -> Vec<Entry> {
		let mut ancestors = Vec::new();
		let mut current = location.parent();
		while let Some(location) = current {
			current = location.parent();
			if let Ok(resolved) = self.resolve(&location) {
				ancestors.push(self.entry(&resolved));
			}
		}
		ancestors.reverse();
		ancestors
	}

	/// The subfolders and media files of a folder, sorted. Each entry is
	/// resolved, so a symlink to the outside is left out.
	fn read_folder(&self, folder: &Resolved) -> Result<Vec<Entry>, Error> {
		let read = fs::read_dir(&folder.path).map_err(|source| io_error(&folder.path, source))?;
		let mut entries = Vec::new();
		for dir_entry in read {
			let dir_entry = dir_entry.map_err(|source| io_error(&folder.path, source))?;
			let Some(name) = dir_entry.file_name().to_str().map(str::to_owned) else {
				let path = dir_entry.path();
				debug!(path = %path.display(), "name is not UTF-8, left out");
				continue;
			};
			if name.starts_with('.') {
				continue;
			}
			let Ok(resolved) = self.resolve(&folder.location.child(&name)) else {
				continue;
			};
			if !resolved.metadata.is_dir() && !is_media_name(&name) {
				continue;
			}
			entries.push(self.entry(&resolved));
		}
		// Folders first, then by name as a user would expect: case does not matter
		entries
			.sort_by_cached_key(|e| (e.kind == Kind::File, e.name.to_lowercase(), e.name.clone()));
		Ok(entries)
	}

	fn entry(&self, resolved: &Resolved) -> Entry {
		let (name, path) = match &resolved.location {
			Location::InRoot { root, relative } => {
				let name = relative
					.file_name()
					.or_else(|| self.roots[*root].file_name())
					.map_or_else(
						|| self.roots[*root].display().to_string(),
						|n| n.to_string_lossy().into_owned(),
					);
				(
					name,
					relative
						.to_string_lossy()
						.replace(std::path::MAIN_SEPARATOR, "/"),
				)
			}
			Location::Absolute(path) => (
				path.file_name().map_or_else(
					|| path.display().to_string(),
					|n| n.to_string_lossy().into_owned(),
				),
				path.display().to_string(),
			),
		};
		let kind = if resolved.metadata.is_dir() {
			Kind::Folder
		} else {
			Kind::File
		};
		Entry {
			id: resolved.location.id(),
			name,
			kind,
			size: if kind == Kind::File {
				resolved.metadata.len()
			} else {
				0
			},
			path,
		}
	}
}

/// A missing path is [`Error::NotFound`]; anything else keeps the OS error.
fn canonicalize(path: &Path) -> Result<PathBuf, Error> {
	fs::canonicalize(path).map_err(|source| io_error(path, source))
}

fn io_error(path: &Path, source: io::Error) -> Error {
	if source.kind() == io::ErrorKind::NotFound {
		Error::NotFound
	} else {
		Error::Io {
			path: path.to_path_buf(),
			source,
		}
	}
}

impl Entry {
	/// The entry's location, for callers that build child ids.
	///
	/// # Errors
	/// Never for an entry the server made; see [`FileId::decode`].
	pub fn location(&self) -> Result<Location, InvalidId> {
		self.id.decode()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs::File;

	/// A media root with a few files, a subfolder and (on Unix) symlinks:
	///
	/// ```text
	/// root/
	///   a.mp4 (5 bytes), b.mkv, notes.txt, .hidden.mp4
	///   series/
	///     s01e01.mkv, s01e02.mkv, cover.jpg
	///     link-inside.mp4 -> ../a.mp4
	///   link-outside.mp4 -> <outside>/secret.mp4
	///   dir-link-outside -> <outside>
	///   dir-link-inside -> series
	/// outside/
	///   secret.mp4
	/// ```
	struct Fixture {
		_dir: tempfile::TempDir,
		root: PathBuf,
		outside: PathBuf,
	}

	fn fixture() -> Fixture {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().join("root");
		let outside = dir.path().join("outside");
		fs::create_dir_all(root.join("series")).unwrap();
		fs::create_dir_all(&outside).unwrap();
		for name in ["b.mkv", "a.mp4", "notes.txt", ".hidden.mp4"] {
			File::create(root.join(name)).unwrap();
		}
		fs::write(root.join("a.mp4"), b"12345").unwrap();
		for name in ["s01e01.mkv", "s01e02.mkv", "cover.jpg"] {
			File::create(root.join("series").join(name)).unwrap();
		}
		File::create(outside.join("secret.mp4")).unwrap();
		#[cfg(unix)]
		{
			use std::os::unix::fs::symlink;
			symlink("../a.mp4", root.join("series/link-inside.mp4")).unwrap();
			symlink(outside.join("secret.mp4"), root.join("link-outside.mp4")).unwrap();
			symlink(&outside, root.join("dir-link-outside")).unwrap();
			symlink(root.join("series"), root.join("dir-link-inside")).unwrap();
		}
		Fixture {
			_dir: dir,
			root,
			outside,
		}
	}

	fn names(entries: &[Entry]) -> Vec<&str> {
		entries.iter().map(|e| e.name.as_str()).collect()
	}

	fn in_root(root: usize, relative: &str) -> FileId {
		Location::InRoot {
			root,
			relative: PathBuf::from(relative),
		}
		.id()
	}

	#[test]
	fn roots_listing_shows_the_configured_folders() {
		let f = fixture();
		let roots = MediaRoots::new(
			vec![f.root.clone(), f.outside.join("missing"), f.outside.clone()],
			false,
		);
		let listing = roots.list(None).unwrap();
		assert!(listing.folder.is_none());
		assert_eq!(listing.ancestors, Vec::new());
		// The missing root is left out, the ids keep the configured index
		assert_eq!(names(&listing.entries), ["root", "outside"]);
		assert_eq!(listing.entries[0].id, in_root(0, ""));
		assert_eq!(listing.entries[1].id, in_root(2, ""));
		assert_eq!(listing.entries[0].kind, Kind::Folder);
		assert_eq!(listing.entries[0].path, "");
	}

	#[test]
	fn listing_shows_folders_then_media_files_sorted() {
		let f = fixture();
		let roots = MediaRoots::new(vec![f.root.clone()], false);
		let listing = roots.list(Some(&in_root(0, ""))).unwrap();
		assert_eq!(listing.folder.as_ref().unwrap().name, "root");
		assert_eq!(listing.ancestors, Vec::new());
		// No notes.txt, no .hidden.mp4, no link to the outside
		#[cfg(unix)]
		assert_eq!(
			names(&listing.entries),
			["dir-link-inside", "series", "a.mp4", "b.mkv"]
		);
		#[cfg(not(unix))]
		assert_eq!(names(&listing.entries), ["series", "a.mp4", "b.mkv"]);
		let a = listing.entries.iter().find(|e| e.name == "a.mp4").unwrap();
		assert_eq!(a.kind, Kind::File);
		assert_eq!(a.size, 5);
		assert_eq!(a.path, "a.mp4");
		assert_eq!(a.id, in_root(0, "a.mp4"));

		let series = roots.list(Some(&in_root(0, "series"))).unwrap();
		assert_eq!(names(&series.ancestors), ["root"]);
		assert_eq!(series.folder.as_ref().unwrap().path, "series");
		#[cfg(unix)]
		assert_eq!(
			names(&series.entries),
			["link-inside.mp4", "s01e01.mkv", "s01e02.mkv"]
		);
		#[cfg(not(unix))]
		assert_eq!(names(&series.entries), ["s01e01.mkv", "s01e02.mkv"]);
		assert_eq!(series.entries.last().unwrap().path, "series/s01e02.mkv");
	}

	#[test]
	fn listing_a_file_or_a_missing_folder_fails() {
		let f = fixture();
		let roots = MediaRoots::new(vec![f.root.clone()], false);
		assert!(matches!(
			roots.list(Some(&in_root(0, "a.mp4"))),
			Err(Error::NotAFolder)
		));
		assert!(matches!(
			roots.list(Some(&in_root(0, "nope"))),
			Err(Error::NotFound)
		));
		assert!(matches!(
			roots.list(Some(&in_root(7, ""))),
			Err(Error::NotFound)
		));
		assert!(matches!(
			roots.list(Some(&FileId::new("@@"))),
			Err(Error::InvalidId(_))
		));
	}

	#[test]
	fn find_matches_wildcards_in_one_folder_or_recursively() {
		let f = fixture();
		let roots = MediaRoots::new(vec![f.root.clone()], false);
		let root = in_root(0, "");
		let found = roots.find(&root, &["*.mp4".into()], false).unwrap();
		assert_eq!(names(&found), ["a.mp4"]);

		let found = roots
			.find(&root, &["*.MP4".into(), "*.mkv".into()], false)
			.unwrap();
		assert_eq!(names(&found), ["a.mp4", "b.mkv"]);

		let found = roots.find(&root, &["*.mkv".into()], true).unwrap();
		assert_eq!(
			found.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
			["b.mkv", "series/s01e01.mkv", "series/s01e02.mkv"]
		);
		assert_eq!(found[1].id, in_root(0, "series/s01e01.mkv"));

		// `*` is every media file, never notes.txt or the hidden file. The
		// symlinked folder is listed but not searched through.
		let found = roots.find(&root, &["*".into()], true).unwrap();
		#[cfg(unix)]
		assert_eq!(
			names(&found),
			[
				"a.mp4",
				"b.mkv",
				"link-inside.mp4",
				"s01e01.mkv",
				"s01e02.mkv"
			]
		);
		#[cfg(not(unix))]
		assert_eq!(
			names(&found),
			["a.mp4", "b.mkv", "s01e01.mkv", "s01e02.mkv"]
		);

		assert_eq!(
			roots.find(&root, &["nothing.*".into()], true).unwrap(),
			Vec::new()
		);
	}

	#[test]
	fn find_rejects_bad_input() {
		let f = fixture();
		let roots = MediaRoots::new(vec![f.root.clone()], false);
		assert!(matches!(
			roots.find(&in_root(0, ""), &[], false),
			Err(Error::NoPatterns)
		));
		assert!(matches!(
			roots.find(&in_root(0, ""), &["[".into()], false),
			Err(Error::Pattern(PatternError::UnclosedSet(0)))
		));
		assert!(matches!(
			roots.find(&in_root(0, "a.mp4"), &["*".into()], false),
			Err(Error::NotAFolder)
		));
	}

	#[test]
	fn get_and_path_follow_the_id() {
		let f = fixture();
		let roots = MediaRoots::new(vec![f.root.clone()], false);
		let entry = roots.get(&in_root(0, "series/s01e01.mkv")).unwrap();
		assert_eq!(entry.name, "s01e01.mkv");
		assert_eq!(entry.kind, Kind::File);
		assert_eq!(
			roots.path(&in_root(0, "series/s01e01.mkv")).unwrap(),
			f.root.canonicalize().unwrap().join("series/s01e01.mkv")
		);
		assert_eq!(roots.get(&in_root(0, "series")).unwrap().kind, Kind::Folder);
		// Exists, but is not media: not something the app can name
		assert!(matches!(
			roots.get(&in_root(0, "notes.txt")),
			Err(Error::NotFound)
		));
		assert!(matches!(
			roots.get(&in_root(0, "gone.mp4")),
			Err(Error::NotFound)
		));
	}

	#[test]
	fn dot_dot_cannot_leave_the_root() {
		let f = fixture();
		let roots = MediaRoots::new(vec![f.root.clone()], false);
		// The id encoding refuses `..` outright
		let crafted = FileId::new(base64::Engine::encode(
			&base64::engine::general_purpose::URL_SAFE_NO_PAD,
			"r0:../outside/secret.mp4",
		));
		assert!(matches!(
			roots.path(&crafted),
			Err(Error::InvalidId(InvalidId::Component))
		));
		// And a location built in code with `..` is caught by the canonical check
		let location = Location::InRoot {
			root: 0,
			relative: PathBuf::from("../outside/secret.mp4"),
		};
		assert!(matches!(roots.resolve(&location), Err(Error::OutsideRoots)));
		// An absolute path is not a relative one
		let crafted = FileId::new(base64::Engine::encode(
			&base64::engine::general_purpose::URL_SAFE_NO_PAD,
			format!("r0:{}", f.outside.join("secret.mp4").display()),
		));
		assert!(matches!(
			roots.path(&crafted),
			Err(Error::InvalidId(InvalidId::Component))
		));
	}

	#[cfg(unix)]
	#[test]
	fn symlinks_cannot_leave_the_root() {
		let f = fixture();
		let roots = MediaRoots::new(vec![f.root.clone()], false);
		assert!(matches!(
			roots.path(&in_root(0, "link-outside.mp4")),
			Err(Error::OutsideRoots)
		));
		assert!(matches!(
			roots.list(Some(&in_root(0, "dir-link-outside"))),
			Err(Error::OutsideRoots)
		));
		assert!(matches!(
			roots.get(&in_root(0, "dir-link-outside/secret.mp4")),
			Err(Error::OutsideRoots)
		));
		// A link that stays inside is fine, and resolves to the real file
		assert_eq!(
			roots.path(&in_root(0, "series/link-inside.mp4")).unwrap(),
			f.root.canonicalize().unwrap().join("a.mp4")
		);
		assert_eq!(
			roots
				.list(Some(&in_root(0, "dir-link-inside")))
				.unwrap()
				.entries
				.len(),
			3
		);
	}

	#[test]
	fn a_sibling_folder_with_the_root_as_prefix_is_outside() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().join("media");
		let sibling = dir.path().join("media2");
		fs::create_dir_all(&root).unwrap();
		fs::create_dir_all(&sibling).unwrap();
		File::create(sibling.join("x.mp4")).unwrap();
		#[cfg(unix)]
		std::os::unix::fs::symlink(sibling.join("x.mp4"), root.join("x.mp4")).unwrap();
		let roots = MediaRoots::new(vec![root], false);
		#[cfg(unix)]
		assert!(matches!(
			roots.path(&in_root(0, "x.mp4")),
			Err(Error::OutsideRoots)
		));
		assert_eq!(
			roots.list(Some(&in_root(0, ""))).unwrap().entries,
			Vec::new()
		);
	}

	#[test]
	fn client_paths_are_only_accepted_in_desktop_mode() {
		let f = fixture();
		let file = f.outside.join("secret.mp4");

		let container = MediaRoots::new(vec![f.root.clone()], false);
		assert!(matches!(
			container.open_path(&file),
			Err(Error::PathsNotAccepted)
		));
		let absolute_id = Location::Absolute(file.clone()).id();
		assert!(matches!(
			container.path(&absolute_id),
			Err(Error::PathsNotAccepted)
		));
		assert!(matches!(
			container.get(&absolute_id),
			Err(Error::PathsNotAccepted)
		));

		let desktop = MediaRoots::new(vec![], true);
		assert!(desktop.accepts_paths());
		let entry = desktop.open_path(&file).unwrap();
		assert_eq!(entry.name, "secret.mp4");
		assert_eq!(entry.kind, Kind::File);
		assert_eq!(entry.path, file.display().to_string());
		assert_eq!(
			desktop.path(&entry.id).unwrap(),
			file.canonicalize().unwrap()
		);

		// A folder from a dialog can then be browsed and searched
		let folder = desktop.open_path(&f.root).unwrap();
		assert_eq!(folder.kind, Kind::Folder);
		let listing = desktop.list(Some(&folder.id)).unwrap();
		assert!(listing.entries.iter().any(|e| e.name == "series"));
		assert_eq!(listing.folder.unwrap().path, f.root.display().to_string());
		let found = desktop.find(&folder.id, &["*.mkv".into()], true).unwrap();
		assert_eq!(found.len(), 3);

		assert!(matches!(
			desktop.open_path(Path::new("relative.mp4")),
			Err(Error::NotAbsolute)
		));
		assert!(matches!(
			desktop.open_path(&f.outside.join("nope.mp4")),
			Err(Error::NotFound)
		));
		assert!(matches!(
			desktop.open_path(&f.root.join("notes.txt")),
			Err(Error::NotFound)
		));
	}

	#[test]
	fn media_names() {
		assert!(is_media_name("a.mp4"));
		assert!(is_media_name("A.MKV"));
		assert!(is_media_name("sub.srt"));
		assert!(!is_media_name("notes.txt"));
		assert!(!is_media_name("mp4"));
		assert!(!is_media_name("archive.mp4.zip"));
	}
}
