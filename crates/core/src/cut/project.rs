//! Project state: a source file's segments and modes, kept on the server
//! with undo and redo.
//!
//! Cutting a long file takes a while, so what the user marked must survive
//! a page reload, a switch to another browser and an app restart. The
//! [`ProjectStore`] keeps one [`ProjectState`] per source file, keyed by
//! the file's path, and writes it to a JSON file in its folder (the
//! server's data folder, never a media folder) on every change. Every
//! change goes through a [`History`], so it can be undone and redone; the
//! history lives in memory for the server run, the state on disk.
//!
//! A project remembers the size and modification time of its source
//! ([`SourceStamp`]) at the last save. Opening it compares those with the
//! file as it is now and reports a [`SourceStatus`]: the segments are
//! returned either way, and the next save (an update, undo or redo)
//! takes the file as it is now.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::{CutMode, Export, Segment, SegmentMode};

/// Folder under the data folder that holds the project files.
pub const PROJECTS_DIR: &str = "projects";
/// How many changes a project's history keeps: older ones fall off.
pub const HISTORY_LIMIT: usize = 200;
/// Version written into every project file, for a later format change.
const FORMAT_VERSION: u32 = 1;
/// How much of the file name is kept in the project file's name.
const STEM_LENGTH: usize = 40;

/// What the cut editor keeps for a source file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectState {
	/// The marked ranges, as the user made them: not normalized.
	pub segments: Vec<Segment>,
	/// Whether the segments are kept or cut away.
	pub segment_mode: SegmentMode,
	/// Where a segment's start lands.
	pub cut_mode: CutMode,
	/// Separate files or one.
	pub export: Export,
}

/// A state with the states before and after it.
///
/// [`push`](Self::push) records a change, [`undo`](Self::undo) and
/// [`redo`](Self::redo) step through them. A change after an undo drops
/// what could have been redone, as every editor does; a change to the
/// same state records nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct History {
	current: ProjectState,
	undo: VecDeque<ProjectState>,
	redo: Vec<ProjectState>,
	limit: usize,
}

impl History {
	/// A history that starts at `current`, keeping at most `limit` states
	/// to undo.
	#[must_use]
	pub fn new(current: ProjectState, limit: usize) -> Self {
		Self {
			current,
			undo: VecDeque::new(),
			redo: Vec::new(),
			limit,
		}
	}

	/// The state now.
	#[must_use]
	pub fn current(&self) -> &ProjectState {
		&self.current
	}

	/// How many states can be undone.
	#[must_use]
	pub fn undo_depth(&self) -> usize {
		self.undo.len()
	}

	/// How many states can be redone.
	#[must_use]
	pub fn redo_depth(&self) -> usize {
		self.redo.len()
	}

	/// Makes `state` the current one, with the previous one to undo.
	/// Returns `false`, changing nothing, when `state` is the current one.
	pub fn push(&mut self, state: ProjectState) -> bool {
		if state == self.current {
			return false;
		}
		let previous = std::mem::replace(&mut self.current, state);
		self.undo.push_back(previous);
		while self.undo.len() > self.limit {
			self.undo.pop_front();
		}
		self.redo.clear();
		true
	}

	/// Goes back to the previous state, or `None` when there is none.
	pub fn undo(&mut self) -> Option<&ProjectState> {
		let previous = self.undo.pop_back()?;
		let undone = std::mem::replace(&mut self.current, previous);
		self.redo.push(undone);
		Some(&self.current)
	}

	/// Goes forward to the last undone state, or `None` when there is
	/// none.
	pub fn redo(&mut self) -> Option<&ProjectState> {
		let next = self.redo.pop()?;
		let redone = std::mem::replace(&mut self.current, next);
		self.undo.push_back(redone);
		Some(&self.current)
	}
}

/// What a project remembers of its source file, to notice a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStamp {
	/// Size in bytes.
	pub size: u64,
	/// Modification time in milliseconds since the Unix epoch, when the
	/// file system reports one.
	pub modified_ms: Option<u64>,
}

impl SourceStamp {
	/// The stamp of the file at `path`, or `None` when there is no file
	/// there (nothing, or a folder).
	///
	/// # Errors
	/// Fails when the file exists but its metadata cannot be read.
	pub fn of(path: &Path) -> io::Result<Option<Self>> {
		let metadata = match std::fs::metadata(path) {
			Ok(metadata) => metadata,
			Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
			Err(error) => return Err(error),
		};
		if !metadata.is_file() {
			return Ok(None);
		}
		let modified_ms = metadata
			.modified()
			.ok()
			.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
			.map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX));
		Ok(Some(Self {
			size: metadata.len(),
			modified_ms,
		}))
	}
}

/// How the source file compares to the last save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
	/// As at the last save, or never saved.
	Unchanged,
	/// Size or modification time differ from the last save.
	Changed,
	/// No file at the source's place.
	Missing,
}

impl SourceStatus {
	/// The status of a file that is `now` for a project that saved it as
	/// `saved`; `None` is a file that was not there.
	#[must_use]
	pub fn compare(saved: Option<SourceStamp>, now: Option<SourceStamp>) -> Self {
		match (saved, now) {
			(_, None) => Self::Missing,
			(Some(saved), Some(now)) if saved != now => Self::Changed,
			_ => Self::Unchanged,
		}
	}
}

/// A project as the store reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
	/// The source file the project belongs to.
	pub source: PathBuf,
	/// The current state; empty for a file without a project.
	pub state: ProjectState,
	/// Whether anything is stored for the file.
	pub saved: bool,
	/// See [`History::undo_depth`].
	pub undo_depth: usize,
	/// See [`History::redo_depth`].
	pub redo_depth: usize,
	/// The source file now, against the last save.
	pub source_status: SourceStatus,
}

/// What can go wrong in the store.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// A file or folder could not be read or written.
	#[error("cannot access {path}: {source}")]
	Io {
		/// The file or folder.
		path: PathBuf,
		/// The underlying error.
		#[source]
		source: io::Error,
	},
	/// A project file holds no valid project.
	#[error("{path} holds no valid project: {source}")]
	Format {
		/// The file.
		path: PathBuf,
		/// The underlying error.
		#[source]
		source: serde_json::Error,
	},
	/// Undo was asked with nothing to undo.
	#[error("nothing to undo")]
	NothingToUndo,
	/// Redo was asked with nothing to redo.
	#[error("nothing to redo")]
	NothingToRedo,
}

/// One open project: its history and what it saved of its source.
#[derive(Debug)]
struct Session {
	history: History,
	stamp: Option<SourceStamp>,
	saved: bool,
}

impl Session {
	fn fresh(limit: usize) -> Self {
		Self {
			history: History::new(ProjectState::default(), limit),
			stamp: None,
			saved: false,
		}
	}

	fn project(&self, source: &Path, source_status: SourceStatus) -> Project {
		Project {
			source: source.to_path_buf(),
			state: self.history.current().clone(),
			saved: self.saved,
			undo_depth: self.history.undo_depth(),
			redo_depth: self.history.redo_depth(),
			source_status,
		}
	}
}

/// Keeps the projects, in memory and optionally in a folder.
#[derive(Debug)]
pub struct ProjectStore {
	/// The folder, or `None` for a store that lives as long as the process.
	dir: Option<PathBuf>,
	history_limit: usize,
	sessions: Mutex<HashMap<PathBuf, Session>>,
}

impl ProjectStore {
	/// A store that forgets everything when the process ends.
	#[must_use]
	pub fn in_memory() -> Self {
		Self::new(None)
	}

	/// A store that writes every project to its own file in `dir`, which is
	/// created on the first write. Projects are read when they are opened.
	#[must_use]
	pub fn in_dir(dir: &Path) -> Self {
		Self::new(Some(dir.to_path_buf()))
	}

	/// [`Self::in_dir`] under [`PROJECTS_DIR`] in the data folder when one is
	/// configured, else [`Self::in_memory`].
	#[must_use]
	pub fn from_config(data_dir: Option<&Path>) -> Self {
		if let Some(dir) = data_dir {
			Self::in_dir(&dir.join(PROJECTS_DIR))
		} else {
			info!("no data folder configured; cut projects are kept in memory only");
			Self::in_memory()
		}
	}

	fn new(dir: Option<PathBuf>) -> Self {
		Self {
			dir,
			history_limit: HISTORY_LIMIT,
			sessions: Mutex::new(HashMap::new()),
		}
	}

	/// Where the projects are written, if anywhere.
	#[must_use]
	pub fn dir(&self) -> Option<&Path> {
		self.dir.as_deref()
	}

	/// The file the project of `source` is written to, if any: the file's
	/// name (shortened) and a hash of its path, so the folder can be read
	/// by a person and a path can be found without listing it.
	#[must_use]
	pub fn project_file(&self, source: &Path) -> Option<PathBuf> {
		self.dir.as_ref().map(|dir| dir.join(file_name(source)))
	}

	/// The project of `source`, read from its file the first time.
	///
	/// # Errors
	/// [`Error::Io`] when the project file or the source's metadata cannot
	/// be read, [`Error::Format`] when the project file is not a project.
	pub async fn open(&self, source: &Path) -> Result<Project, Error> {
		let mut sessions = self.sessions.lock().await;
		let session = self.session(&mut sessions, source).await?;
		let status = status(session, source)?;
		Ok(session.project(source, status))
	}

	/// Makes `state` the current state of `source`'s project and saves it.
	/// A state equal to the current one changes nothing, but is saved when
	/// the project never was.
	///
	/// # Errors
	/// As [`open`](Self::open), plus the project file not being writable.
	pub async fn update(&self, source: &Path, state: ProjectState) -> Result<Project, Error> {
		let mut sessions = self.sessions.lock().await;
		let session = self.session(&mut sessions, source).await?;
		let changed = session.history.push(state);
		if changed || !session.saved {
			self.save(session, source).await?;
		}
		let status = status(session, source)?;
		Ok(session.project(source, status))
	}

	/// Goes back one change in `source`'s project and saves the result.
	///
	/// # Errors
	/// [`Error::NothingToUndo`], else as [`update`](Self::update).
	pub async fn undo(&self, source: &Path) -> Result<Project, Error> {
		let mut sessions = self.sessions.lock().await;
		let session = self.session(&mut sessions, source).await?;
		session.history.undo().ok_or(Error::NothingToUndo)?;
		self.save(session, source).await?;
		let status = status(session, source)?;
		Ok(session.project(source, status))
	}

	/// Reapplies the last undone change in `source`'s project and saves
	/// the result.
	///
	/// # Errors
	/// [`Error::NothingToRedo`], else as [`update`](Self::update).
	pub async fn redo(&self, source: &Path) -> Result<Project, Error> {
		let mut sessions = self.sessions.lock().await;
		let session = self.session(&mut sessions, source).await?;
		session.history.redo().ok_or(Error::NothingToRedo)?;
		self.save(session, source).await?;
		let status = status(session, source)?;
		Ok(session.project(source, status))
	}

	/// The session of `source`, loaded from its file when it is not open.
	async fn session<'a>(
		&self,
		sessions: &'a mut HashMap<PathBuf, Session>,
		source: &Path,
	) -> Result<&'a mut Session, Error> {
		if !sessions.contains_key(source) {
			let session = match self.project_file(source) {
				Some(path) => self.load(&path, source).await?,
				None => Session::fresh(self.history_limit),
			};
			sessions.insert(source.to_path_buf(), session);
		}
		// Just inserted when it was missing.
		sessions.get_mut(source).ok_or_else(|| Error::Io {
			path: source.to_path_buf(),
			source: io::Error::other("the project vanished while it was opened"),
		})
	}

	/// The session in the project file at `path`, or a fresh one when there
	/// is none or it belongs to another source (a hash collision, which is
	/// logged: the other project is overwritten on the next save).
	async fn load(&self, path: &Path, source: &Path) -> Result<Session, Error> {
		let bytes = match tokio::fs::read(path).await {
			Ok(bytes) => bytes,
			Err(error) if error.kind() == io::ErrorKind::NotFound => {
				return Ok(Session::fresh(self.history_limit));
			}
			Err(source) => {
				return Err(Error::Io {
					path: path.to_path_buf(),
					source,
				});
			}
		};
		let stored: StoredProject =
			serde_json::from_slice(&bytes).map_err(|source| Error::Format {
				path: path.to_path_buf(),
				source,
			})?;
		if stored.source.path != source.to_string_lossy() {
			warn!(
				file = %path.display(),
				stored = %stored.source.path,
				source = %source.display(),
				"project file belongs to another source; starting a new project"
			);
			return Ok(Session::fresh(self.history_limit));
		}
		Ok(Session {
			history: History::new(stored.state.into(), self.history_limit),
			stamp: stored.source.stamp,
			saved: true,
		})
	}

	/// Writes the session's state to the project file (if any) and takes
	/// the source's stamp when the file is there.
	async fn save(&self, session: &mut Session, source: &Path) -> Result<(), Error> {
		if let Ok(Some(stamp)) = SourceStamp::of(source) {
			session.stamp = Some(stamp);
		}
		if let Some(path) = self.project_file(source) {
			let stored = StoredProject {
				version: FORMAT_VERSION,
				source: StoredSource {
					path: source.to_string_lossy().into_owned(),
					stamp: session.stamp,
				},
				state: session.history.current().clone().into(),
			};
			write_atomically(&path, &stored).await?;
		}
		session.saved = true;
		Ok(())
	}
}

/// The status of `source` against what `session` saved of it.
fn status(session: &Session, source: &Path) -> Result<SourceStatus, Error> {
	let now = SourceStamp::of(source).map_err(|error| Error::Io {
		path: source.to_path_buf(),
		source: error,
	})?;
	Ok(SourceStatus::compare(session.stamp, now))
}

/// `<stem>-<hash>.json`: up to [`STEM_LENGTH`] plain characters of the
/// file's name, and the FNV-1a hash of its whole path.
fn file_name(source: &Path) -> String {
	let stem: String = source
		.file_stem()
		.map(|stem| stem.to_string_lossy())
		.unwrap_or_default()
		.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() || c == '.' {
				c
			} else {
				'_'
			}
		})
		.take(STEM_LENGTH)
		.collect();
	let hash = fnv1a(source.to_string_lossy().as_bytes());
	if stem.is_empty() {
		format!("{hash:016x}.json")
	} else {
		format!("{stem}-{hash:016x}.json")
	}
}

/// 64-bit FNV-1a: stable across builds, unlike `DefaultHasher`.
fn fnv1a(bytes: &[u8]) -> u64 {
	bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
		(hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
	})
}

/// Serialises `stored` and writes it with [`crate::fs::write_atomically`],
/// so a crash mid-write leaves the old file intact.
async fn write_atomically(path: &Path, stored: &StoredProject) -> Result<(), Error> {
	let json = serde_json::to_vec_pretty(stored).map_err(|source| Error::Format {
		path: path.to_path_buf(),
		source,
	})?;
	crate::fs::write_atomically(path, json)
		.await
		.map_err(|source| Error::Io {
			path: path.to_path_buf(),
			source,
		})
}

/// The project file.
#[derive(Debug, Serialize, Deserialize)]
struct StoredProject {
	version: u32,
	source: StoredSource,
	state: StoredState,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredSource {
	path: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	stamp: Option<SourceStamp>,
}

/// [`ProjectState`] as written: the cut types in the file's own words, so
/// the file format does not follow the Rust names.
#[derive(Debug, Serialize, Deserialize)]
struct StoredState {
	segments: Vec<StoredSegment>,
	segment_mode: StoredSegmentMode,
	cut_mode: StoredCutMode,
	export: StoredExport,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredSegment {
	start: f64,
	end: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredSegmentMode {
	Keep,
	Remove,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredCutMode {
	Keyframe,
	Normal,
	Smart,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredExport {
	SeparateFiles,
	Merged,
}

impl From<ProjectState> for StoredState {
	fn from(state: ProjectState) -> Self {
		Self {
			segments: state
				.segments
				.iter()
				.map(|s| StoredSegment {
					start: s.start,
					end: s.end,
				})
				.collect(),
			segment_mode: match state.segment_mode {
				SegmentMode::Keep => StoredSegmentMode::Keep,
				SegmentMode::Remove => StoredSegmentMode::Remove,
			},
			cut_mode: match state.cut_mode {
				CutMode::Keyframe => StoredCutMode::Keyframe,
				CutMode::Normal => StoredCutMode::Normal,
				CutMode::Smart => StoredCutMode::Smart,
			},
			export: match state.export {
				Export::SeparateFiles => StoredExport::SeparateFiles,
				Export::Merged => StoredExport::Merged,
			},
		}
	}
}

impl From<StoredState> for ProjectState {
	fn from(stored: StoredState) -> Self {
		Self {
			segments: stored
				.segments
				.iter()
				.map(|s| Segment::new(s.start, s.end))
				.collect(),
			segment_mode: match stored.segment_mode {
				StoredSegmentMode::Keep => SegmentMode::Keep,
				StoredSegmentMode::Remove => SegmentMode::Remove,
			},
			cut_mode: match stored.cut_mode {
				StoredCutMode::Keyframe => CutMode::Keyframe,
				StoredCutMode::Normal => CutMode::Normal,
				StoredCutMode::Smart => CutMode::Smart,
			},
			export: match stored.export {
				StoredExport::SeparateFiles => Export::SeparateFiles,
				StoredExport::Merged => Export::Merged,
			},
		}
	}
}

#[cfg(test)]
mod tests {
	use std::time::{Duration, SystemTime};

	use proptest::prelude::*;

	use super::*;

	fn state(ranges: &[(f64, f64)]) -> ProjectState {
		ProjectState {
			segments: ranges.iter().map(|(s, e)| Segment::new(*s, *e)).collect(),
			..ProjectState::default()
		}
	}

	#[test]
	fn history_undoes_and_redoes_in_order() {
		let mut history = History::new(state(&[]), HISTORY_LIMIT);
		assert!(history.push(state(&[(1.0, 2.0)])));
		assert!(history.push(state(&[(1.0, 2.0), (3.0, 4.0)])));
		assert_eq!((history.undo_depth(), history.redo_depth()), (2, 0));

		assert_eq!(history.undo(), Some(&state(&[(1.0, 2.0)])));
		assert_eq!(history.undo(), Some(&state(&[])));
		assert_eq!(history.undo(), None);
		assert_eq!((history.undo_depth(), history.redo_depth()), (0, 2));

		assert_eq!(history.redo(), Some(&state(&[(1.0, 2.0)])));
		assert_eq!(history.redo(), Some(&state(&[(1.0, 2.0), (3.0, 4.0)])));
		assert_eq!(history.redo(), None);
		assert_eq!(history.current(), &state(&[(1.0, 2.0), (3.0, 4.0)]));
	}

	#[test]
	fn a_change_after_an_undo_drops_the_redo_stack() {
		let mut history = History::new(state(&[]), HISTORY_LIMIT);
		history.push(state(&[(1.0, 2.0)]));
		history.push(state(&[(5.0, 6.0)]));
		history.undo();
		assert_eq!(history.redo_depth(), 1);
		assert!(history.push(state(&[(7.0, 8.0)])));
		assert_eq!(history.redo_depth(), 0);
		assert_eq!(history.redo(), None);
		assert_eq!(history.undo(), Some(&state(&[(1.0, 2.0)])));
	}

	#[test]
	fn pushing_the_current_state_records_nothing() {
		let mut history = History::new(state(&[(1.0, 2.0)]), HISTORY_LIMIT);
		assert!(!history.push(state(&[(1.0, 2.0)])));
		assert_eq!(history.undo_depth(), 0);
		// A mode change is a change.
		assert!(history.push(ProjectState {
			segment_mode: SegmentMode::Remove,
			..state(&[(1.0, 2.0)])
		}));
		assert_eq!(history.undo_depth(), 1);
	}

	#[test]
	fn the_history_is_bounded_and_drops_the_oldest() {
		let mut history = History::new(state(&[]), 3);
		for i in 1..=5 {
			history.push(state(&[(f64::from(i), f64::from(i) + 1.0)]));
		}
		assert_eq!(history.undo_depth(), 3);
		assert_eq!(history.undo(), Some(&state(&[(4.0, 5.0)])));
		assert_eq!(history.undo(), Some(&state(&[(3.0, 4.0)])));
		assert_eq!(history.undo(), Some(&state(&[(2.0, 3.0)])));
		assert_eq!(history.undo(), None);
	}

	/// A random walk of pushes, undos and redos.
	#[derive(Debug, Clone)]
	enum Step {
		Push(u16),
		Undo,
		Redo,
	}

	fn steps() -> impl Strategy<Value = Vec<Step>> {
		prop::collection::vec(
			prop_oneof![
				(0u16..1000).prop_map(Step::Push),
				Just(Step::Undo),
				Just(Step::Redo),
			],
			0..40,
		)
	}

	proptest! {
		#[test]
		fn the_history_matches_a_cursor_over_a_list(steps in steps(), limit in 1usize..6) {
			let mut history = History::new(state(&[]), limit);
			// The model: every state ever current, and a cursor.
			let mut states = vec![state(&[])];
			let mut cursor = 0;
			for step in steps {
				match step {
					Step::Push(n) => {
						let next = state(&[(f64::from(n), f64::from(n) + 1.0)]);
						if next == states[cursor] {
							prop_assert!(!history.push(next));
						} else {
							states.truncate(cursor + 1);
							states.push(next);
							cursor += 1;
							// Older states than the limit fall off the front.
							if cursor > limit {
								states.remove(0);
								cursor -= 1;
							}
							prop_assert!(history.push(states[cursor].clone()));
						}
					}
					Step::Undo => {
						if cursor > 0 {
							cursor -= 1;
							prop_assert_eq!(history.undo(), Some(&states[cursor]));
						} else {
							prop_assert_eq!(history.undo(), None);
						}
					}
					Step::Redo => {
						if cursor + 1 < states.len() {
							cursor += 1;
							prop_assert_eq!(history.redo(), Some(&states[cursor]));
						} else {
							prop_assert_eq!(history.redo(), None);
						}
					}
				}
				prop_assert_eq!(history.current(), &states[cursor]);
				prop_assert_eq!(history.undo_depth(), cursor);
				prop_assert_eq!(history.redo_depth(), states.len() - cursor - 1);
				prop_assert!(history.undo_depth() <= limit);
			}
		}
	}

	#[test]
	fn source_status_compares_stamps() {
		let a = SourceStamp {
			size: 10,
			modified_ms: Some(1),
		};
		let b = SourceStamp {
			size: 10,
			modified_ms: Some(2),
		};
		assert_eq!(SourceStatus::compare(None, None), SourceStatus::Missing);
		assert_eq!(SourceStatus::compare(Some(a), None), SourceStatus::Missing);
		assert_eq!(
			SourceStatus::compare(None, Some(a)),
			SourceStatus::Unchanged
		);
		assert_eq!(
			SourceStatus::compare(Some(a), Some(a)),
			SourceStatus::Unchanged
		);
		assert_eq!(
			SourceStatus::compare(Some(a), Some(b)),
			SourceStatus::Changed
		);
	}

	#[test]
	fn project_file_names_are_readable_stable_and_distinct() {
		let store = ProjectStore::in_dir(Path::new("/data/projects"));
		let name = store
			.project_file(Path::new("/media/Movie A (2024).mkv"))
			.unwrap();
		let name = name.file_name().unwrap().to_string_lossy().into_owned();
		assert!(name.starts_with("Movie_A__2024_-"), "{name}");
		assert_eq!(
			Path::new(&name).extension().and_then(|e| e.to_str()),
			Some("json")
		);
		assert_eq!(name.len(), "Movie_A__2024_-".len() + 16 + ".json".len());
		assert_eq!(
			store.project_file(Path::new("/media/Movie A (2024).mkv")),
			store.project_file(Path::new("/media/Movie A (2024).mkv"))
		);
		assert_ne!(
			store.project_file(Path::new("/media/a/clip.mkv")),
			store.project_file(Path::new("/media/b/clip.mkv"))
		);
		let long = "x".repeat(100);
		let name = store.project_file(Path::new(&format!("/m/{long}.mkv")));
		assert!(name.unwrap().file_name().unwrap().len() < 80);
		assert!(
			ProjectStore::in_memory()
				.project_file(Path::new("/m/a.mkv"))
				.is_none()
		);
		assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
		assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
	}

	#[tokio::test]
	async fn an_in_memory_store_keeps_projects_for_the_run() {
		let store = ProjectStore::in_memory();
		let source = Path::new("/nowhere/clip.mkv");
		let empty = store.open(source).await.unwrap();
		assert!(!empty.saved);
		assert_eq!(empty.state, ProjectState::default());
		assert_eq!(empty.source_status, SourceStatus::Missing);

		let updated = store.update(source, state(&[(1.0, 2.0)])).await.unwrap();
		assert!(updated.saved);
		assert_eq!((updated.undo_depth, updated.redo_depth), (1, 0));
		let undone = store.undo(source).await.unwrap();
		assert_eq!(undone.state, ProjectState::default());
		assert_eq!((undone.undo_depth, undone.redo_depth), (0, 1));
		assert!(matches!(
			store.undo(source).await,
			Err(Error::NothingToUndo)
		));
		let redone = store.redo(source).await.unwrap();
		assert_eq!(redone.state, state(&[(1.0, 2.0)]));
		assert!(matches!(
			store.redo(source).await,
			Err(Error::NothingToRedo)
		));
		assert_eq!(
			store.open(source).await.unwrap().state,
			state(&[(1.0, 2.0)])
		);
	}

	#[tokio::test]
	async fn a_file_store_survives_a_reopen_without_its_history() {
		let dir = tempfile::tempdir().unwrap();
		let source = dir.path().join("clip.mkv");
		std::fs::write(&source, b"not really a video").unwrap();
		let projects = dir.path().join("data/projects");

		let store = ProjectStore::in_dir(&projects);
		let first = state(&[(1.0, 2.0)]);
		let second = ProjectState {
			segment_mode: SegmentMode::Remove,
			cut_mode: CutMode::Normal,
			export: Export::Merged,
			..state(&[(1.0, 2.0), (3.5, 4.25)])
		};
		store.update(&source, first.clone()).await.unwrap();
		let saved = store.update(&source, second.clone()).await.unwrap();
		assert_eq!(saved.source_status, SourceStatus::Unchanged);
		let file = store.project_file(&source).unwrap();
		assert!(file.is_file());
		let entries: Vec<_> = std::fs::read_dir(&projects)
			.unwrap()
			.map(|e| e.unwrap().path())
			.collect();
		assert_eq!(
			entries,
			std::slice::from_ref(&file),
			"no temp file is left behind"
		);

		// What a server restart does: a new store over the same folder.
		let reopened = ProjectStore::in_dir(&projects);
		let project = reopened.open(&source).await.unwrap();
		assert!(project.saved);
		assert_eq!(project.state, second);
		assert_eq!(project.source_status, SourceStatus::Unchanged);
		assert_eq!((project.undo_depth, project.redo_depth), (0, 0));

		// The undone state is what is on disk afterwards.
		store.undo(&source).await.unwrap();
		let after_undo = ProjectStore::in_dir(&projects).open(&source).await.unwrap();
		assert_eq!(after_undo.state, first);
	}

	#[tokio::test]
	async fn a_changed_or_missing_source_is_reported_until_the_next_save() {
		let dir = tempfile::tempdir().unwrap();
		let source = dir.path().join("clip.mkv");
		std::fs::write(&source, b"version one").unwrap();
		let store = ProjectStore::in_dir(&dir.path().join("projects"));
		store.update(&source, state(&[(1.0, 2.0)])).await.unwrap();

		// Same size, other modification time: changed.
		let file = std::fs::File::options().write(true).open(&source).unwrap();
		file.set_modified(SystemTime::now() + Duration::from_secs(120))
			.unwrap();
		drop(file);
		let project = store.open(&source).await.unwrap();
		assert_eq!(project.source_status, SourceStatus::Changed);
		assert_eq!(project.state, state(&[(1.0, 2.0)]));
		// Every save, an undo included, accepts the file as it is now.
		assert_eq!(
			store.undo(&source).await.unwrap().source_status,
			SourceStatus::Unchanged
		);
		assert_eq!(
			store.redo(&source).await.unwrap().source_status,
			SourceStatus::Unchanged
		);
		let saved = store.update(&source, state(&[(1.0, 3.0)])).await.unwrap();
		assert_eq!(saved.source_status, SourceStatus::Unchanged);

		// Other size: changed, also across a restart.
		std::fs::write(&source, b"version two is longer").unwrap();
		let restarted = ProjectStore::in_dir(&dir.path().join("projects"));
		assert_eq!(
			restarted.open(&source).await.unwrap().source_status,
			SourceStatus::Changed
		);

		// Gone: missing, and the segments are still there.
		std::fs::remove_file(&source).unwrap();
		let project = restarted.open(&source).await.unwrap();
		assert_eq!(project.source_status, SourceStatus::Missing);
		assert_eq!(project.state, state(&[(1.0, 3.0)]));
		// A folder in its place is no file either.
		std::fs::create_dir(&source).unwrap();
		assert_eq!(
			restarted.open(&source).await.unwrap().source_status,
			SourceStatus::Missing
		);
	}

	#[tokio::test]
	async fn a_broken_or_foreign_project_file() {
		let dir = tempfile::tempdir().unwrap();
		let store = ProjectStore::in_dir(dir.path());
		let source = Path::new("/media/clip.mkv");
		let file = store.project_file(source).unwrap();
		std::fs::write(&file, b"{ not json").unwrap();
		assert!(matches!(
			store.open(source).await,
			Err(Error::Format { .. })
		));

		// A file for another path under this name (a hash collision) is
		// not this project.
		let other = ProjectStore::in_dir(dir.path());
		std::fs::write(
			&file,
			serde_json::to_vec(&StoredProject {
				version: FORMAT_VERSION,
				source: StoredSource {
					path: "/media/other.mkv".into(),
					stamp: None,
				},
				state: state(&[(1.0, 2.0)]).into(),
			})
			.unwrap(),
		)
		.unwrap();
		let project = other.open(source).await.unwrap();
		assert!(!project.saved);
		assert_eq!(project.state, ProjectState::default());
	}

	#[test]
	fn the_json_names_are_stable() {
		let stored = StoredProject {
			version: FORMAT_VERSION,
			source: StoredSource {
				path: "/media/clip.mkv".into(),
				stamp: Some(SourceStamp {
					size: 1234,
					modified_ms: Some(1_700_000_000_000),
				}),
			},
			state: ProjectState {
				segments: vec![Segment::new(1.5, 2.0)],
				segment_mode: SegmentMode::Remove,
				cut_mode: CutMode::Normal,
				export: Export::Merged,
			}
			.into(),
		};
		assert_eq!(
			serde_json::to_value(&stored).unwrap(),
			serde_json::json!({
				"version": 1,
				"source": {
					"path": "/media/clip.mkv",
					"stamp": { "size": 1234, "modified_ms": 1_700_000_000_000_u64 }
				},
				"state": {
					"segments": [{ "start": 1.5, "end": 2.0 }],
					"segment_mode": "remove",
					"cut_mode": "normal",
					"export": "merged"
				}
			})
		);
		let back: StoredProject = serde_json::from_value(serde_json::json!({
			"version": 1,
			"source": { "path": "/media/clip.mkv" },
			"state": {
				"segments": [],
				"segment_mode": "keep",
				"cut_mode": "keyframe",
				"export": "separate_files"
			}
		}))
		.unwrap();
		assert_eq!(back.source.stamp, None);
		assert_eq!(ProjectState::from(back.state), ProjectState::default());
	}
}
