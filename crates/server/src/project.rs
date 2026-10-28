//! The project methods of `CutService`: the saved segment state of a
//! source file, with undo and redo.
//!
//! The logic is `muxmultitool_core::cut::project`: this module resolves
//! the source file (which, unlike a cut's input, may have disappeared),
//! validates the state and maps the core types to the messages. Projects
//! are stored in [`AppState::projects`](crate::AppState::projects), under
//! the data folder when one is configured.

use std::path::{Path, PathBuf};

use connectrpc::ConnectError;
use muxmultitool_core::cut::project::{self, Project, ProjectState, SourceStatus};
use muxmultitool_core::cut::{CutMode, Export, Segment, SegmentMode};
use muxmultitool_core::files::{FileId, Location};
use muxmultitool_proto::proto::muxmultitool::cut::v1::{self as proto, CutExport, CutSegment};

use crate::cut::finite;
use crate::jobs::Jobs;

/// `GetProject`: the project of `file_id`, read from its file the first
/// time.
pub(crate) async fn get(jobs: &Jobs, file_id: &str) -> Result<proto::Project, ConnectError> {
	let source = jobs.project_source(file_id)?;
	let project = jobs.state.projects.open(&source).await;
	Ok(message(&project.map_err(|error| store_error(&error))?))
}

/// `UpdateProject`: `state` becomes the project's current state.
pub(crate) async fn update(
	jobs: &Jobs,
	file_id: &str,
	state: Option<proto::ProjectState>,
) -> Result<proto::Project, ConnectError> {
	let source = jobs.project_source(file_id)?;
	let state = state_from_message(state.as_ref())?;
	let project = jobs.state.projects.update(&source, state).await;
	Ok(message(&project.map_err(|error| store_error(&error))?))
}

/// `UndoProject`.
pub(crate) async fn undo(jobs: &Jobs, file_id: &str) -> Result<proto::Project, ConnectError> {
	let source = jobs.project_source(file_id)?;
	let project = jobs.state.projects.undo(&source).await;
	Ok(message(&project.map_err(|error| store_error(&error))?))
}

/// `RedoProject`.
pub(crate) async fn redo(jobs: &Jobs, file_id: &str) -> Result<proto::Project, ConnectError> {
	let source = jobs.project_source(file_id)?;
	let project = jobs.state.projects.redo(&source).await;
	Ok(message(&project.map_err(|error| store_error(&error))?))
}

impl Jobs {
	/// The path a project is keyed by: the file behind an id from
	/// `FilesService`, or an absolute path inside a media root, as a
	/// canonical path. The file itself may be gone (a project outlives its
	/// source, and reports it), but its folder must exist and be inside
	/// the roots; a desktop path from a native dialog needs no root.
	pub(crate) fn project_source(&self, id_or_path: &str) -> Result<PathBuf, ConnectError> {
		if id_or_path.is_empty() {
			return Err(ConnectError::invalid_argument("file_id is required"));
		}
		let id = FileId::new(id_or_path);
		if let Ok(path) = self.state.files.path(&id) {
			if !path.is_file() {
				return Err(ConnectError::invalid_argument(format!(
					"file_id is not a file: {}",
					path.display()
				)));
			}
			return Ok(path);
		}
		if let Ok(location) = id.decode() {
			return match location {
				Location::InRoot { root, relative } => {
					let root = self.state.files.roots().get(root).ok_or_else(|| {
						ConnectError::not_found("file_id names a media root that is not configured")
					})?;
					self.file_place(&root.join(relative), true)
				}
				Location::Absolute(path) => {
					if !self.state.files.accepts_paths() {
						return Err(ConnectError::permission_denied(
							"paths from the client are only accepted in desktop mode",
						));
					}
					self.file_place(&path, false)
				}
			};
		}
		let path = Path::new(id_or_path);
		if !path.is_absolute() {
			return Err(ConnectError::invalid_argument(format!(
				"file_id must be an id or an absolute path: {id_or_path}"
			)));
		}
		self.file_place(path, true)
	}

	/// `path` as the canonical path of its folder plus its name, for a
	/// file that need not exist. The folder must (`NOT_FOUND` otherwise) and,
	/// with `check_roots`, must be inside the media roots.
	fn file_place(&self, path: &Path, check_roots: bool) -> Result<PathBuf, ConnectError> {
		let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
			return Err(ConnectError::invalid_argument(format!(
				"file_id must name a file: {}",
				path.display()
			)));
		};
		let folder = parent.canonicalize().map_err(|_| {
			ConnectError::not_found(format!("folder not found: {}", parent.display()))
		})?;
		if check_roots {
			self.inside_media_roots(&folder, "file_id")?;
		}
		let place = folder.join(name);
		if place.is_dir() {
			return Err(ConnectError::invalid_argument(format!(
				"file_id is not a file: {}",
				place.display()
			)));
		}
		Ok(place)
	}
}

fn store_error(error: &project::Error) -> ConnectError {
	match error {
		project::Error::NothingToUndo | project::Error::NothingToRedo => {
			ConnectError::failed_precondition(error.to_string())
		}
		project::Error::Format { .. } => ConnectError::data_loss(error.to_string()),
		project::Error::Io { .. } => ConnectError::internal(error.to_string()),
	}
}

/// The core state of a message; `None` is an empty state.
fn state_from_message(state: Option<&proto::ProjectState>) -> Result<ProjectState, ConnectError> {
	let Some(state) = state else {
		return Ok(ProjectState::default());
	};
	let segment_mode = match state.segment_mode.as_known() {
		Some(proto::SegmentMode::Unspecified | proto::SegmentMode::Keep) => SegmentMode::Keep,
		Some(proto::SegmentMode::Remove) => SegmentMode::Remove,
		None => {
			return Err(ConnectError::invalid_argument(format!(
				"unknown segment mode {}",
				state.segment_mode.to_i32()
			)));
		}
	};
	let cut_mode = match state.cut_mode.as_known() {
		Some(proto::CutMode::Unspecified | proto::CutMode::Keyframe) => CutMode::Keyframe,
		Some(proto::CutMode::Normal) => CutMode::Normal,
		None => {
			return Err(ConnectError::invalid_argument(format!(
				"unknown cut mode {}",
				state.cut_mode.to_i32()
			)));
		}
	};
	let export = match state.export.as_known() {
		Some(CutExport::Unspecified | CutExport::SeparateFiles) => Export::SeparateFiles,
		Some(CutExport::Merged) => Export::Merged,
		None => {
			return Err(ConnectError::invalid_argument(format!(
				"unknown export {}",
				state.export.to_i32()
			)));
		}
	};
	let segments = state
		.segments
		.iter()
		.map(|s| {
			Ok(Segment::new(
				finite(s.start_seconds, "start_seconds")?,
				finite(s.end_seconds, "end_seconds")?,
			))
		})
		.collect::<Result<Vec<_>, ConnectError>>()?;
	Ok(ProjectState {
		segments,
		segment_mode,
		cut_mode,
		export,
	})
}

fn state_message(state: &ProjectState) -> proto::ProjectState {
	proto::ProjectState {
		segments: state
			.segments
			.iter()
			.map(|s| CutSegment {
				start_seconds: s.start,
				end_seconds: s.end,
				..Default::default()
			})
			.collect(),
		segment_mode: match state.segment_mode {
			SegmentMode::Keep => proto::SegmentMode::Keep,
			SegmentMode::Remove => proto::SegmentMode::Remove,
		}
		.into(),
		cut_mode: match state.cut_mode {
			CutMode::Keyframe => proto::CutMode::Keyframe,
			CutMode::Normal => proto::CutMode::Normal,
		}
		.into(),
		export: match state.export {
			Export::SeparateFiles => CutExport::SeparateFiles,
			Export::Merged => CutExport::Merged,
		}
		.into(),
		..Default::default()
	}
}

fn message(project: &Project) -> proto::Project {
	let depth = |depth: usize| u32::try_from(depth).unwrap_or(u32::MAX);
	proto::Project {
		state: Some(state_message(&project.state)).into(),
		saved: project.saved,
		undo_depth: depth(project.undo_depth),
		redo_depth: depth(project.redo_depth),
		source_status: match project.source_status {
			SourceStatus::Unchanged => proto::SourceStatus::Unchanged,
			SourceStatus::Changed => proto::SourceStatus::Changed,
			SourceStatus::Missing => proto::SourceStatus::Missing,
		}
		.into(),
		source_path: project.source.to_string_lossy().into_owned(),
		..Default::default()
	}
}
