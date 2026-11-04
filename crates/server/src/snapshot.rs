//! The frame route and the snapshot and cover-art operations of
//! `JobsService`.
//!
//! `GET /media/{id}/frame?t=12.5[&format=jpg|png]` answers one picture
//! decoded from the *original* file, at full resolution: the id is a
//! preview session (the id `StartPreview` returned), whose source file is
//! used, or a file id. The preview stream itself is never read, because it
//! is scaled and re-encoded. The logic is `muxmultitool_core::snapshot`.
//!
//! Saving a snapshot and setting the cover art are jobs (`SubmitJob`);
//! this module turns the two operations into [`JobSpec`]s. The cover art
//! needs the frame as a file for the remux to read, so it is grabbed at
//! submit time into a temporary folder that lives until the job ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Path as RoutePath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use connectrpc::{ConnectError, ErrorCode};
use muxmultitool_core::ffmpeg::Limits;
use muxmultitool_core::ffmpeg::probe::{CodecType, ProbeResult};
use muxmultitool_core::jobs::{JobId, JobSpec, Queue};
use muxmultitool_core::snapshot::{self, GrabError, ImageFormat, TempImage, command, plan_cover};
use muxmultitool_proto::proto::muxmultitool::snapshot::v1::{CoverArtOperation, SnapshotOperation};
use serde::Deserialize;
use tokio::sync::Semaphore;
use tracing::warn;

use crate::jobs::{Jobs, PROBE_TIMEOUT, output_near};
use crate::{AppState, timeline};

/// How many frames the route decodes at once. Scrubbing sends a request per
/// pointer move; the rest wait their turn instead of starting a decoder each.
const CONCURRENT_FRAMES: usize = 3;

static FRAME_SLOTS: Semaphore = Semaphore::const_new(CONCURRENT_FRAMES);

/// The frame route, to merge into the app router.
pub(crate) fn routes() -> Router<Arc<AppState>> {
	Router::new().route("/media/{id}/frame", get(frame))
}

async fn frame(
	State(state): State<Arc<AppState>>,
	RoutePath(id): RoutePath<String>,
	Query(query): Query<FrameQuery>,
) -> Response {
	let position = match query.position() {
		Ok(position) => position,
		Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
	};
	let format = match query.format() {
		Ok(format) => format,
		Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
	};
	let source = match state.previews.source(&id) {
		Some(source) => source,
		None => match timeline::resolve(&state, &id).await {
			Ok(source) => source,
			Err(error) => return (status_of(error.code), error.to_string()).into_response(),
		},
	};
	let Ok(_slot) = FRAME_SLOTS.acquire().await else {
		return StatusCode::SERVICE_UNAVAILABLE.into_response();
	};
	match snapshot::grab(&state.runner, &source, position, format).await {
		Ok(bytes) => (
			[
				(header::CONTENT_TYPE, format.mime()),
				// The same id, position and format always give the same
				// picture, unless the file is replaced
				(header::CACHE_CONTROL, "private, max-age=300"),
			],
			bytes,
		)
			.into_response(),
		Err(error @ GrabError::NoFrame(_)) => {
			(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response()
		}
		Err(error) => {
			warn!(id, %error, "cannot grab the frame");
			(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
		}
	}
}

fn status_of(code: ErrorCode) -> StatusCode {
	match code {
		ErrorCode::NotFound => StatusCode::NOT_FOUND,
		ErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
		ErrorCode::InvalidArgument => StatusCode::BAD_REQUEST,
		_ => StatusCode::INTERNAL_SERVER_ERROR,
	}
}

/// The route's query string: `t` (seconds) and `format`. A missing `t`
/// or a `t` that is not a number is a `400` from the extractor itself.
#[derive(Debug, Deserialize)]
struct FrameQuery {
	t: f64,
	format: Option<String>,
}

impl FrameQuery {
	fn position(&self) -> Result<Duration, &'static str> {
		Duration::try_from_secs_f64(self.t)
			.map_err(|_| "t must be a number of seconds, zero or more")
	}

	/// JPEG unless the query says otherwise.
	fn format(&self) -> Result<ImageFormat, &'static str> {
		self.format
			.as_deref()
			.map_or(Ok(ImageFormat::Jpeg), |name| {
				ImageFormat::from_name(name).ok_or("format must be jpg or png")
			})
	}
}

impl Jobs {
	/// Validates a snapshot request and turns it into a job spec: the
	/// output must be a `.jpg`, `.jpeg` or `.png` file, the input must have
	/// video, and the position must be inside it.
	pub(crate) async fn snapshot_spec(
		&self,
		name: &str,
		operation: &SnapshotOperation,
	) -> Result<JobSpec, ConnectError> {
		let input = self.cut_input(&operation.input)?;
		let output = self.checked_output(
			&output_near(&input, &operation.output, |input| {
				input.with_extension("jpg")
			})
			.to_string_lossy(),
		)?;
		let format = ImageFormat::from_path(&output).ok_or_else(|| {
			ConnectError::invalid_argument(
				"a snapshot is saved as .jpg, .jpeg or .png; the output has another extension",
			)
		})?;
		let probe = self.probe_for_frame(&input).await?;
		let position = checked_position(operation.position_ms, &probe)?;
		Ok(JobSpec {
			name: job_name(name, &output),
			command: command::frame(&input, &output, position, format),
			expected_duration: None,
			preparation: Vec::new(),
			scratch_dir: None,
			outputs: vec![output],
		})
	}

	/// Validates a cover-art request, grabs the frame and turns the remux
	/// into a job spec. The returned picture must outlive the job; see
	/// [`hold_until_done`].
	pub(crate) async fn cover_art_spec(
		&self,
		name: &str,
		operation: &CoverArtOperation,
	) -> Result<(JobSpec, TempImage), ConnectError> {
		let input = self.cut_input(&operation.input)?;
		let output = self.checked_output(
			&output_near(&input, &operation.output, default_cover_output).to_string_lossy(),
		)?;
		let probe = self.probe_for_frame(&input).await?;
		let plan = plan_cover(&probe, &output)
			.map_err(|error| ConnectError::failed_precondition(error.to_string()))?;
		let position = checked_position(operation.position_ms, &probe)?;
		let picture =
			snapshot::grab_to_temp(&self.state.runner, &input, position, ImageFormat::Jpeg)
				.await
				.map_err(|error| match error {
					GrabError::NoFrame(_) => ConnectError::invalid_argument(error.to_string()),
					_ => ConnectError::failed_precondition(error.to_string()),
				})?;
		let spec = JobSpec {
			name: job_name(name, &output),
			command: command::cover(&input, picture.path(), &output, &plan),
			expected_duration: probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
			preparation: Vec::new(),
			scratch_dir: None,
			outputs: vec![output],
		};
		Ok((spec, picture))
	}

	/// Probes `input` and checks that it has a picture to take a frame from.
	async fn probe_for_frame(&self, input: &Path) -> Result<ProbeResult, ConnectError> {
		let probe = self
			.state
			.runner
			.probe(input, &Limits::timeout(PROBE_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::invalid_argument(format!("the input is not a media file: {error}"))
			})?;
		let has_video = probe
			.streams
			.iter()
			.any(|s| s.codec_type == CodecType::Video && !s.disposition.attached_pic);
		if has_video {
			Ok(probe)
		} else {
			Err(ConnectError::failed_precondition(
				"the input has no video stream to take a frame from",
			))
		}
	}
}

/// The position as a duration, inside the input when its length is known.
fn checked_position(position_ms: i64, probe: &ProbeResult) -> Result<Duration, ConnectError> {
	let position = u64::try_from(position_ms)
		.map(Duration::from_millis)
		.map_err(|_| ConnectError::invalid_argument("position_ms must not be negative"))?;
	if let Some(length) = probe
		.duration_secs()
		.and_then(|secs| Duration::try_from_secs_f64(secs).ok())
		&& position >= length
	{
		return Err(ConnectError::invalid_argument(format!(
			"position_ms is at or after the end of the input ({} ms)",
			length.as_millis()
		)));
	}
	Ok(position)
}

/// `movie.cover.mkv` next to `movie.mkv`: the same container, so the copy
/// keeps every stream.
fn default_cover_output(input: &Path) -> PathBuf {
	let stem = input
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_default();
	let name = match input.extension() {
		Some(ext) => format!("{stem}.cover.{}", ext.to_string_lossy()),
		None => format!("{stem}.cover"),
	};
	input.with_file_name(name)
}

/// The job's label: `name`, or the output file name when it is empty.
fn job_name(name: &str, output: &Path) -> String {
	if name.is_empty() {
		output
			.file_name()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_default()
	} else {
		name.to_owned()
	}
}

/// Keeps `picture` (and its folder) until the job `id` has ended, then
/// removes it. The watch also ends when the queue shuts down.
pub(crate) fn hold_until_done(queue: &Queue, id: &JobId, picture: TempImage) {
	let Ok(mut watch) = queue.watch_job(id, 0) else {
		return;
	};
	tokio::spawn(async move {
		while watch.next().await.is_some() {}
		drop(picture);
	});
}

#[cfg(test)]
mod tests {
	use super::*;

	fn query(t: f64, format: Option<&str>) -> FrameQuery {
		FrameQuery {
			t,
			format: format.map(str::to_owned),
		}
	}

	#[test]
	fn the_query_needs_a_position_and_defaults_to_jpeg() {
		assert_eq!(
			query(12.5, None).position(),
			Ok(Duration::from_millis(12_500))
		);
		assert_eq!(query(12.5, None).format(), Ok(ImageFormat::Jpeg));
		assert!(query(-1.0, None).position().is_err());
		assert!(query(f64::NAN, None).position().is_err());
		assert!(query(f64::INFINITY, None).position().is_err());
		assert_eq!(query(0.0, Some("PNG")).format(), Ok(ImageFormat::Png));
		assert!(query(1.0, Some("gif")).format().is_err());
	}
}
