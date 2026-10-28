//! `FilesService`: browses the media roots and hands out opaque file ids.
//!
//! The logic is [`MediaRoots`] in `core`; this module maps requests to it
//! and its errors to Connect codes. The file system calls block, so each
//! method runs them on tokio's blocking pool.

use std::path::PathBuf;
use std::sync::Arc;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::files::{self, Entry, FileId, Kind, MediaRoots};
use muxmultitool_proto::connect::muxmultitool::files::v1::FilesService;
use muxmultitool_proto::proto::muxmultitool::files::v1::{
	self as pb, FindFilesRequest, FindFilesResponse, GetEntriesRequest, GetEntriesResponse,
	GetStreamDetailsRequest, GetStreamDetailsResponse, GetStreamReportRequest,
	GetStreamReportResponse, ListFolderRequest, ListFolderResponse, OpenPathsRequest,
	OpenPathsResponse,
};
use tracing::warn;

use crate::AppState;

/// Implements the files service on top of the state's [`MediaRoots`].
#[derive(Debug)]
pub(crate) struct Files {
	state: Arc<AppState>,
}

impl Files {
	pub(crate) fn new(state: Arc<AppState>) -> Self {
		Self { state }
	}

	/// Runs `work` against the media roots on the blocking pool.
	async fn blocking<T: Send + 'static>(
		&self,
		work: impl FnOnce(&MediaRoots) -> Result<T, files::Error> + Send + 'static,
	) -> ServiceResult<T> {
		let state = Arc::clone(&self.state);
		let result = tokio::task::spawn_blocking(move || work(&state.files))
			.await
			.map_err(|error| {
				warn!(%error, "files task failed");
				ConnectError::internal("the files task failed")
			})?;
		result.map(Response::new).map_err(connect_error)
	}
}

impl FilesService for Files {
	async fn list_folder(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, ListFolderRequest>,
	) -> ServiceResult<ListFolderResponse> {
		let folder_id = request.to_owned_message().folder_id;
		self.blocking(move |roots| {
			let folder = (!folder_id.is_empty()).then(|| FileId::new(folder_id));
			let listing = roots.list(folder.as_ref())?;
			Ok(ListFolderResponse {
				folder: listing.folder.map(entry).into(),
				ancestors: listing.ancestors.into_iter().map(entry).collect(),
				entries: listing.entries.into_iter().map(entry).collect(),
				..Default::default()
			})
		})
		.await
	}

	async fn find_files(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, FindFilesRequest>,
	) -> ServiceResult<FindFilesResponse> {
		let request = request.to_owned_message();
		self.blocking(move |roots| {
			let folder = FileId::new(request.folder_id);
			let files = roots.find(&folder, &request.patterns, request.recursive)?;
			Ok(FindFilesResponse {
				files: files.into_iter().map(entry).collect(),
				..Default::default()
			})
		})
		.await
	}

	async fn get_entries(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, GetEntriesRequest>,
	) -> ServiceResult<GetEntriesResponse> {
		let ids = request.to_owned_message().ids;
		self.blocking(move |roots| {
			let entries = ids
				.into_iter()
				.map(|id| match roots.get(&FileId::new(&*id)) {
					Ok(found) => entry(found),
					// An id that no longer resolves: the id alone, kind unspecified
					Err(_) => pb::Entry {
						id,
						..Default::default()
					},
				})
				.collect();
			Ok(GetEntriesResponse {
				entries,
				..Default::default()
			})
		})
		.await
	}

	async fn open_paths(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, OpenPathsRequest>,
	) -> ServiceResult<OpenPathsResponse> {
		let paths = request.to_owned_message().paths;
		self.blocking(move |roots| {
			let entries = paths
				.iter()
				.map(|path| roots.open_path(&PathBuf::from(path)).map(entry))
				.collect::<Result<_, _>>()?;
			Ok(OpenPathsResponse {
				entries,
				..Default::default()
			})
		})
		.await
	}

	async fn get_stream_report(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, GetStreamReportRequest>,
	) -> ServiceResult<GetStreamReportResponse> {
		crate::report::stream_report(&self.state, request.to_owned_message()).await
	}

	async fn get_stream_details(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, GetStreamDetailsRequest>,
	) -> ServiceResult<GetStreamDetailsResponse> {
		crate::report::stream_details(&self.state, request.to_owned_message()).await
	}
}

pub(crate) fn entry(entry: Entry) -> pb::Entry {
	pb::Entry {
		id: entry.id.as_str().to_owned(),
		name: entry.name,
		kind: match entry.kind {
			Kind::Folder => pb::EntryKind::ENTRY_KIND_FOLDER,
			Kind::File => pb::EntryKind::ENTRY_KIND_FILE,
		}
		.into(),
		size_bytes: entry.size,
		path: entry.path,
		..Default::default()
	}
}

/// The Connect code for each files error. The message is the error's own
/// text: it names what was wrong, never a server path.
pub(crate) fn connect_error(error: files::Error) -> ConnectError {
	let message = error.to_string();
	match error {
		files::Error::InvalidId(_)
		| files::Error::NotAFolder
		| files::Error::NotAbsolute
		| files::Error::Pattern(_)
		| files::Error::NoPatterns => ConnectError::invalid_argument(message),
		files::Error::NotFound => ConnectError::not_found(message),
		files::Error::OutsideRoots | files::Error::PathsNotAccepted => {
			ConnectError::permission_denied(message)
		}
		files::Error::Io { path, source } => {
			warn!(path = %path.display(), error = %source, "files request failed");
			if source.kind() == std::io::ErrorKind::PermissionDenied {
				ConnectError::permission_denied("the server may not read that folder")
			} else {
				ConnectError::internal("the folder could not be read")
			}
		}
	}
}
