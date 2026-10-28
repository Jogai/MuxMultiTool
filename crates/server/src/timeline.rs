//! The timeline part of `MediaService` and the sprite route.
//!
//! Thumbnails, waveform peaks and subtitle cues are background analyses in
//! core (`TimelineManager`). Each RPC here is a server stream that wraps a
//! core watch: what is known at once, then every addition until the
//! analysis ends. The sprite images are the one thing the browser fetches
//! by URL, so they are a plain `GET` route:
//!
//! - `GET /media/{id}/thumbs/{n}.jpg`: sprite `n` of the cached file `id`,
//!   as `GetThumbnails` announced it, sent by `tower-http`'s `ServeFile`
//!   (`Range`, `Last-Modified`, `Content-Length`). Cacheable for a day: a
//!   sprite's content for a given id and number never changes, and a
//!   changed file gets a new id. `404` for a sprite that is not ready, or
//!   an unknown id.
//!
//! File ids: an opaque id from `FilesService`, or the absolute path that
//! `StartPreview` accepts for now (see [`crate::media`]).

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path as RoutePath, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use connectrpc::{ConnectError, Response as RpcResponse, ServiceResult, ServiceStream};
use muxmultitool_core::files::{self, FileId};
use muxmultitool_core::timeline::{
	AnalysisError, CueUpdate, ThumbnailUpdate, TimelineError, WaveformUpdate,
};
use muxmultitool_proto::proto::muxmultitool::media::v1::{
	GetSubtitleCuesRequest, GetSubtitleCuesResponse, GetThumbnailsRequest, GetThumbnailsResponse,
	GetWaveformRequest, GetWaveformResponse, SubtitleCue, ThumbnailSheet,
};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use tracing::warn;

use crate::AppState;
use crate::media::resolve_file;

pub(crate) async fn get_thumbnails(
	state: &Arc<AppState>,
	request: GetThumbnailsRequest,
) -> ServiceResult<ServiceStream<GetThumbnailsResponse>> {
	let source = resolve(state, &request.file_id).await?;
	let watch = state
		.timelines
		.thumbnails(&source)
		.await
		.map_err(|error| timeline_error(&error))?;
	Ok(RpcResponse::stream(futures_util::stream::unfold(
		watch,
		|mut watch| async move {
			let item = watch.next().await?;
			Some((
				item.map(thumbnails_message)
					.map_err(|error| analysis_error(&error)),
				watch,
			))
		},
	)))
}

pub(crate) async fn get_waveform(
	state: &Arc<AppState>,
	request: GetWaveformRequest,
) -> ServiceResult<ServiceStream<GetWaveformResponse>> {
	let source = resolve(state, &request.file_id).await?;
	let count = usize::try_from(request.peak_count).unwrap_or(usize::MAX);
	let watch = state
		.timelines
		.waveform(
			&source,
			request.stream_index,
			request.from_seconds,
			request.to_seconds,
			count,
		)
		.await
		.map_err(|error| timeline_error(&error))?;
	Ok(RpcResponse::stream(futures_util::stream::unfold(
		watch,
		|mut watch| async move {
			let item = watch.next().await?;
			Some((
				item.as_ref().map(waveform_message).map_err(analysis_error),
				watch,
			))
		},
	)))
}

pub(crate) async fn get_subtitle_cues(
	state: &Arc<AppState>,
	request: GetSubtitleCuesRequest,
) -> ServiceResult<ServiceStream<GetSubtitleCuesResponse>> {
	let source = resolve(state, &request.file_id).await?;
	let watch = state
		.timelines
		.subtitle_cues(&source, request.stream_index)
		.await
		.map_err(|error| timeline_error(&error))?;
	Ok(RpcResponse::stream(futures_util::stream::unfold(
		watch,
		|mut watch| async move {
			let item = watch.next().await?;
			Some((
				item.map(cues_message)
					.map_err(|error| analysis_error(&error)),
				watch,
			))
		},
	)))
}

fn thumbnails_message(update: ThumbnailUpdate) -> GetThumbnailsResponse {
	let sheet = update.sheet;
	GetThumbnailsResponse {
		sheet: ThumbnailSheet {
			sprite_url_template: format!("/media/{}/thumbs/{{n}}.jpg", update.media_id),
			media_id: update.media_id,
			interval_seconds: sheet.interval_secs,
			tile_width: sheet.tile_width,
			tile_height: sheet.tile_height,
			columns: sheet.columns,
			rows: sheet.rows,
			tile_count: sheet.tile_count,
			sprite_count: sheet.sprite_count,
			keyframes_only: sheet.keyframes_only,
			..Default::default()
		}
		.into(),
		sprites_ready: u32::try_from(update.ready).unwrap_or(u32::MAX),
		done: update.done,
		..Default::default()
	}
}

fn waveform_message(update: &WaveformUpdate) -> GetWaveformResponse {
	GetWaveformResponse {
		from_seconds: update.window.from,
		to_seconds: update.window.to,
		peak_count: u32::try_from(update.window.count).unwrap_or(u32::MAX),
		offset: u32::try_from(update.offset).unwrap_or(u32::MAX),
		mins: update.peaks.iter().map(|p| p.min).collect(),
		maxs: update.peaks.iter().map(|p| p.max).collect(),
		analyzed_seconds: update.analyzed_secs,
		done: update.done,
		..Default::default()
	}
}

fn cues_message(update: CueUpdate) -> GetSubtitleCuesResponse {
	GetSubtitleCuesResponse {
		cues: update
			.cues
			.into_iter()
			.map(|cue| SubtitleCue {
				start_seconds: cue.start,
				end_seconds: cue.end,
				text: cue.text,
				..Default::default()
			})
			.collect(),
		done: update.done,
		..Default::default()
	}
}

/// Turns a file id into a path: an opaque id through the media roots, or
/// the absolute-path form `StartPreview` takes.
async fn resolve(state: &Arc<AppState>, file_id: &str) -> Result<PathBuf, ConnectError> {
	let id = FileId::new(file_id);
	if id.decode().is_err() {
		return resolve_file(&state.media_roots, file_id).await;
	}
	let state = Arc::clone(state);
	tokio::task::spawn_blocking(move || state.files.path(&id))
		.await
		.map_err(|error| {
			warn!(%error, "files task failed");
			ConnectError::internal("the files task failed")
		})?
		.map_err(|error| {
			let message = error.to_string();
			match error {
				files::Error::NotFound => ConnectError::not_found(message),
				files::Error::OutsideRoots | files::Error::PathsNotAccepted => {
					ConnectError::permission_denied(message)
				}
				files::Error::Io { .. } => ConnectError::internal("the file could not be read"),
				_ => ConnectError::invalid_argument(message),
			}
		})
}

fn timeline_error(error: &TimelineError) -> ConnectError {
	let message = error.to_string();
	match error {
		TimelineError::Open { .. } | TimelineError::NotAFile(_) | TimelineError::NoStream(_) => {
			ConnectError::not_found(message)
		}
		TimelineError::InvalidWindow(_) => ConnectError::invalid_argument(message),
		TimelineError::Probe(_)
		| TimelineError::NoDuration
		| TimelineError::NoVideo
		| TimelineError::NotAudio(_)
		| TimelineError::NotSubtitle(_)
		| TimelineError::ImageSubtitles { .. } => ConnectError::failed_precondition(message),
		TimelineError::Io(_) => ConnectError::internal(message),
	}
}

/// The error that ends a stream whose analysis did not finish.
fn analysis_error(error: &AnalysisError) -> ConnectError {
	match error {
		AnalysisError::Failed(_) => ConnectError::internal(error.to_string()),
		AnalysisError::Cancelled => ConnectError::unavailable(error.to_string()),
	}
}

/// The sprite route, to merge into the app router.
pub(crate) fn routes() -> Router<Arc<AppState>> {
	Router::new().route("/media/{id}/thumbs/{sprite}", get(sprite))
}

async fn sprite(
	State(state): State<Arc<AppState>>,
	RoutePath((id, name)): RoutePath<(String, String)>,
	request: Request,
) -> Response {
	let Some(n) = name
		.strip_suffix(".jpg")
		.and_then(|n| n.parse::<usize>().ok())
	else {
		return StatusCode::NOT_FOUND.into_response();
	};
	let Some(path) = state.timelines.sprite(&id, n) else {
		return StatusCode::NOT_FOUND.into_response();
	};
	let mut response = match ServeFile::new(&path).oneshot(request).await {
		Ok(response) => response.into_response(),
		Err(error) => match error {},
	};
	if response.status().is_success() {
		let headers = response.headers_mut();
		headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
		headers.insert(
			header::CACHE_CONTROL,
			HeaderValue::from_static("private, max-age=86400, immutable"),
		);
	}
	response
}
