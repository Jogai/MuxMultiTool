//! `MediaService` and the plain HTTP media routes.
//!
//! The RPC starts and stops preview sessions (`PreviewManager` in core)
//! and returns the playlist path. The browser then fetches the playlist and
//! the segments itself, so those are ordinary `GET` routes next to the RPC
//! mount:
//!
//! - `GET /media/{id}/hls/index.m3u8`: the playlist, never cached because
//!   the id can be reused after a restart;
//! - `GET /media/{id}/hls/{n}.ts`: a segment, produced on demand. The
//!   request waits while ffmpeg makes it, then `tower-http`'s `ServeFile`
//!   sends it (`Range`, `Last-Modified`, `Content-Length`). Cacheable: a
//!   segment's content for a given id and number does not change.
//!
//! The timeline methods of the service (thumbnails, waveform, subtitle
//! cues) and the sprite route are in [`crate::timeline`]; the trait
//! implementation here only delegates to them.
//!
//! File ids: until `FilesService` issues ids, a file id is the absolute path
//! of a file inside one of the configured media roots. [`resolve_file`] is
//! the seam where real ids plug in.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path as RoutePath, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use connectrpc::{
	ConnectError, RequestContext, Response as RpcResponse, ServiceRequest, ServiceResult,
	ServiceStream,
};
use muxmultitool_core::preview::{Mode, PreviewError, SegmentError};
use muxmultitool_proto::connect::muxmultitool::media::v1::MediaService;
use muxmultitool_proto::proto::muxmultitool::media::v1::{
	GetSubtitleCuesRequest, GetSubtitleCuesResponse, GetThumbnailsRequest, GetThumbnailsResponse,
	GetWaveformRequest, GetWaveformResponse, PreviewMode, StartPreviewRequest,
	StartPreviewResponse, StopPreviewRequest, StopPreviewResponse,
};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use tracing::warn;

use crate::{AppState, timeline};

/// Implements the media service.
#[derive(Debug)]
pub(crate) struct Media {
	pub(crate) state: Arc<AppState>,
}

impl MediaService for Media {
	async fn start_preview(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, StartPreviewRequest>,
	) -> ServiceResult<StartPreviewResponse> {
		// An opaque id from FilesService, or the absolute-path form (see resolve_file)
		let source = timeline::resolve(&self.state, request.file_id).await?;
		let info = self
			.state
			.previews
			.start(&source)
			.await
			.map_err(|error| match error {
				PreviewError::Open { .. } | PreviewError::NotAFile(_) => {
					ConnectError::not_found(error.to_string())
				}
				PreviewError::NoStreams | PreviewError::NoDuration => {
					ConnectError::failed_precondition(error.to_string())
				}
				PreviewError::Probe(_) => ConnectError::failed_precondition(error.to_string()),
				PreviewError::Io(_) => ConnectError::internal(error.to_string()),
			})?;
		Ok(RpcResponse::new(StartPreviewResponse {
			playlist_url: playlist_path(&info.id),
			media_id: info.id,
			duration_seconds: info.duration,
			mode: match info.mode {
				Mode::Remux => PreviewMode::PREVIEW_MODE_REMUX,
				Mode::TranscodeAudio => PreviewMode::PREVIEW_MODE_TRANSCODE_AUDIO,
				Mode::Transcode => PreviewMode::PREVIEW_MODE_TRANSCODE,
			}
			.into(),
			segment_count: u32::try_from(info.segment_count).unwrap_or(u32::MAX),
			..Default::default()
		}))
	}

	async fn stop_preview(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, StopPreviewRequest>,
	) -> ServiceResult<StopPreviewResponse> {
		self.state.previews.stop(request.media_id).await;
		Ok(RpcResponse::new(StopPreviewResponse::default()))
	}

	async fn get_thumbnails(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, GetThumbnailsRequest>,
	) -> ServiceResult<ServiceStream<GetThumbnailsResponse>> {
		timeline::get_thumbnails(&self.state, request.to_owned_message()).await
	}

	async fn get_waveform(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, GetWaveformRequest>,
	) -> ServiceResult<ServiceStream<GetWaveformResponse>> {
		timeline::get_waveform(&self.state, request.to_owned_message()).await
	}

	async fn get_subtitle_cues(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, GetSubtitleCuesRequest>,
	) -> ServiceResult<ServiceStream<GetSubtitleCuesResponse>> {
		timeline::get_subtitle_cues(&self.state, request.to_owned_message()).await
	}
}

/// The playlist path of session `id`, relative to the server origin.
fn playlist_path(id: &str) -> String {
	format!("/media/{id}/hls/index.m3u8")
}

/// Turns a file id into a path the preview may read.
///
/// For now the id is the absolute path of a file under a media root; the
/// path is canonicalized before the check, so `..` and symlinks cannot
/// leave the roots. Desktop mode, where native dialogs hand out paths
/// outside any root, is not covered yet; it needs the ids from
/// `FilesService`.
pub(crate) async fn resolve_file(
	media_roots: &[PathBuf],
	file_id: &str,
) -> Result<PathBuf, ConnectError> {
	if media_roots.is_empty() {
		return Err(ConnectError::failed_precondition(
			"no media roots are configured",
		));
	}
	let path = Path::new(file_id);
	if file_id.is_empty() || !path.is_absolute() {
		return Err(ConnectError::invalid_argument(
			"file_id must be the absolute path of a file inside a media root",
		));
	}
	let canonical = tokio::fs::canonicalize(path)
		.await
		.map_err(|error| ConnectError::not_found(format!("{file_id}: {error}")))?;
	for root in media_roots {
		let Ok(root) = tokio::fs::canonicalize(root).await else {
			continue;
		};
		if canonical.starts_with(&root) {
			return Ok(canonical);
		}
	}
	Err(ConnectError::permission_denied(format!(
		"{file_id} is outside the media roots"
	)))
}

/// The media routes, to merge into the app router.
pub(crate) fn routes() -> Router<Arc<AppState>> {
	Router::new()
		.route("/media/{id}/hls/index.m3u8", get(playlist))
		.route("/media/{id}/hls/{segment}", get(segment))
}

async fn playlist(
	State(state): State<Arc<AppState>>,
	RoutePath(id): RoutePath<String>,
) -> Response {
	match state.previews.playlist(&id) {
		Some(text) => (
			[
				(header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
				(header::CACHE_CONTROL, "no-cache"),
			],
			text,
		)
			.into_response(),
		None => StatusCode::NOT_FOUND.into_response(),
	}
}

async fn segment(
	State(state): State<Arc<AppState>>,
	RoutePath((id, name)): RoutePath<(String, String)>,
	request: Request,
) -> Response {
	let Some(n) = name
		.strip_suffix(".ts")
		.and_then(|n| n.parse::<usize>().ok())
	else {
		return StatusCode::NOT_FOUND.into_response();
	};
	let path = match state.previews.segment(&id, n).await {
		Ok(path) => path,
		Err(
			error @ (SegmentError::UnknownSession(_)
			| SegmentError::OutOfRange(_)
			| SegmentError::NotProduced(_)),
		) => {
			return (StatusCode::NOT_FOUND, error.to_string()).into_response();
		}
		Err(error @ SegmentError::Timeout { .. }) => {
			warn!(id, n, %error, "segment not ready");
			return (StatusCode::GATEWAY_TIMEOUT, error.to_string()).into_response();
		}
		Err(error @ SegmentError::Failed(_)) => {
			return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
		}
	};
	let mut response = match ServeFile::new(&path).oneshot(request).await {
		Ok(response) => response.into_response(),
		Err(error) => match error {},
	};
	if response.status().is_success() {
		let headers = response.headers_mut();
		headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("video/mp2t"));
		headers.insert(
			header::CACHE_CONTROL,
			HeaderValue::from_static("private, max-age=3600"),
		);
	}
	response
}
