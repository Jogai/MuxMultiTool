//! `GifService` and the GIF operations of `JobsService`.
//!
//! Both conversions run as jobs (`SubmitJob` with a `VideoToGifOperation` or
//! a `GifToVideoOperation`); this module turns those operations into
//! [`JobSpec`]s. The logic is `muxmultitool_core::gif`: the server only
//! resolves paths, probes the input, asks the capability cache whether the
//! build has what the job needs, and maps the core types to the messages.
//!
//! `EstimateGif` runs the same plan and limit check as a submit, without
//! queueing anything, so the frontend can show the estimate and the
//! violations while the user adjusts the range, width and limits.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::ffmpeg::command::ShellLine;
use muxmultitool_core::ffmpeg::{Capabilities, Limits, ProbeResult};
use muxmultitool_core::gif::{self, Limit, Plan, Quality, Request, Source, VideoFormat, Violation};
use muxmultitool_core::jobs::JobSpec;
use muxmultitool_proto::connect::muxmultitool::gif::v1::GifService;
use muxmultitool_proto::proto::muxmultitool::gif::v1::{
	self as proto, EstimateGifRequest, EstimateGifResponse, GifQuality, GifToVideoOperation,
	GifVideoFormat, VideoToGifOperation,
};

use crate::AppState;
use crate::jobs::{Jobs, PROBE_TIMEOUT, output_near};

/// Implements the GIF service. It reuses the jobs service's path checks and
/// spec builders, so an estimate and a submit go through the same code.
#[derive(Debug)]
pub(crate) struct Gif {
	jobs: Jobs,
}

impl Gif {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}
}

impl GifService for Gif {
	async fn estimate_gif(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, EstimateGifRequest>,
	) -> ServiceResult<EstimateGifResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation.into_option() else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let input = self.jobs.cut_input(&operation.input)?;
		// The output is only shown in the command here; it need not exist.
		let output = output_near(&input, &operation.output, default_gif_output);
		let plan = self.jobs.gif_plan(&operation, &input).await?;
		let violations = plan.violations(&limits_of(&operation));
		let argv = self
			.jobs
			.state
			.runner
			.ffmpeg_invocation(&plan.command(&input, &output));
		Ok(Response::new(EstimateGifResponse {
			estimate: Some(estimate_message(&plan)).into(),
			violations: violations.iter().map(violation_message).collect(),
			command: ShellLine(&argv).to_string(),
			..Default::default()
		}))
	}
}

impl Jobs {
	/// Validates a video → GIF request and turns it into a job spec. The
	/// build must have the GIF encoder and the palette filters, and the plan
	/// must be within its limits; otherwise the submit fails with
	/// `FAILED_PRECONDITION` and the violations' messages.
	pub(crate) async fn video_to_gif_spec(
		&self,
		name: &str,
		operation: &VideoToGifOperation,
	) -> Result<JobSpec, ConnectError> {
		let input = self.cut_input(&operation.input)?;
		let output = self.checked_output(
			&output_near(&input, &operation.output, default_gif_output).to_string_lossy(),
		)?;
		let caps = self.capabilities().await?;
		let missing = gif::missing_for_gif(&caps);
		if !missing.is_empty() {
			return Err(ConnectError::failed_precondition(format!(
				"this ffmpeg build cannot make GIFs: it lacks {}",
				missing.join(", ")
			)));
		}
		let plan = self.gif_plan(operation, &input).await?;
		let violations = plan.violations(&limits_of(operation));
		if !violations.is_empty() {
			let messages: Vec<String> = violations.iter().map(ToString::to_string).collect();
			return Err(ConnectError::failed_precondition(format!(
				"the GIF would be over its limits: {}",
				messages.join("; ")
			)));
		}
		Ok(JobSpec {
			name: job_name(name, &output),
			command: plan.command(&input, &output),
			expected_duration: Some(plan.duration),
			preparation: Vec::new(),
			scratch_dir: None,
		})
	}

	/// Validates a GIF → video request and turns it into a job spec, with
	/// the best encoder the build has for the format.
	pub(crate) async fn gif_to_video_spec(
		&self,
		name: &str,
		operation: &GifToVideoOperation,
	) -> Result<JobSpec, ConnectError> {
		let input = self.cut_input(&operation.input)?;
		let format = match operation.format.as_known() {
			Some(GifVideoFormat::Unspecified | GifVideoFormat::Mp4) => VideoFormat::Mp4,
			Some(GifVideoFormat::Webm) => VideoFormat::Webm,
			None => {
				return Err(ConnectError::invalid_argument(format!(
					"unknown video format {}",
					operation.format.to_i32()
				)));
			}
		};
		let extension = match format {
			VideoFormat::Mp4 => "mp4",
			VideoFormat::Webm => "webm",
		};
		let output = self.checked_output(
			&output_near(&input, &operation.output, |input| {
				input.with_extension(extension)
			})
			.to_string_lossy(),
		)?;
		let caps = self.capabilities().await?;
		let Some(encoder) = format.encoder(&caps) else {
			let names: Vec<&str> = format.encoders().iter().map(|e| e.name()).collect();
			return Err(ConnectError::failed_precondition(format!(
				"this ffmpeg build cannot write {format}: it lacks the {} encoder",
				names.join(" or ")
			)));
		};
		let probe = self.probe_input(&input).await?;
		if !probe.format.format_name.split(',').any(|n| n == "gif") {
			return Err(ConnectError::invalid_argument(format!(
				"the input is not a GIF but {}: {}",
				probe.format.format_name,
				input.display()
			)));
		}
		Ok(JobSpec {
			name: job_name(name, &output),
			command: gif::command::gif_to_video(&input, &output, encoder),
			expected_duration: probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
			preparation: Vec::new(),
			scratch_dir: None,
		})
	}

	/// Probes `input` and resolves `operation` against its video stream.
	async fn gif_plan(
		&self,
		operation: &VideoToGifOperation,
		input: &Path,
	) -> Result<Plan, ConnectError> {
		let probe = self.probe_input(input).await?;
		let Some(source) = Source::from_probe(&probe) else {
			return Err(ConnectError::invalid_argument(format!(
				"the input has no video stream: {}",
				input.display()
			)));
		};
		let quality = match operation.quality.as_known() {
			Some(GifQuality::Unspecified | GifQuality::Balanced) => Quality::Balanced,
			Some(GifQuality::Small) => Quality::Small,
			Some(GifQuality::Best) => Quality::Best,
			None => {
				return Err(ConnectError::invalid_argument(format!(
					"unknown quality {}",
					operation.quality.to_i32()
				)));
			}
		};
		let request = Request {
			start: millis(operation.start_ms, "start_ms")?,
			end: millis(operation.end_ms, "end_ms")?,
			fps: operation.fps,
			width: operation.width,
			quality,
			play_once: operation.play_once,
		};
		gif::plan(&request, &source)
			.map_err(|error| ConnectError::invalid_argument(error.to_string()))
	}

	async fn probe_input(&self, input: &Path) -> Result<ProbeResult, ConnectError> {
		self.state
			.runner
			.probe(input, &Limits::timeout(PROBE_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::invalid_argument(format!("the input is not a media file: {error}"))
			})
	}

	async fn capabilities(&self) -> Result<Arc<Capabilities>, ConnectError> {
		self.state
			.capabilities
			.get(&self.state.runner)
			.await
			.map_err(|error| {
				ConnectError::failed_precondition(format!(
					"ffmpeg is missing or broken, so its capabilities are unknown: {error}"
				))
			})
	}
}

/// The limits of a request: the message's fields, with the defaults for
/// the zero ones and for a missing message.
fn limits_of(operation: &VideoToGifOperation) -> gif::Limits {
	let defaults = gif::Limits::default();
	let Some(limits) = operation.limits.as_option() else {
		return defaults;
	};
	gif::Limits {
		max_duration: u64::try_from(limits.max_duration_ms)
			.ok()
			.filter(|ms| *ms > 0)
			.map_or(defaults.max_duration, Duration::from_millis),
		max_width: if limits.max_width == 0 {
			defaults.max_width
		} else {
			limits.max_width
		},
		max_height: (limits.max_height > 0).then_some(limits.max_height),
		max_bytes: if limits.max_bytes == 0 {
			defaults.max_bytes
		} else {
			limits.max_bytes
		},
	}
}

/// A non-negative millisecond field as a duration.
fn millis(ms: i64, field: &str) -> Result<Duration, ConnectError> {
	u64::try_from(ms)
		.map(Duration::from_millis)
		.map_err(|_| ConnectError::invalid_argument(format!("{field} must not be negative")))
}

/// `movie.gif` next to `movie.mkv`.
fn default_gif_output(input: &Path) -> PathBuf {
	input.with_extension("gif")
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

fn estimate_message(plan: &Plan) -> proto::GifEstimate {
	proto::GifEstimate {
		width: plan.width,
		height: plan.height,
		fps: plan.fps,
		duration_ms: i64::try_from(plan.duration.as_millis()).unwrap_or(i64::MAX),
		frames: plan.frames,
		estimated_bytes: plan.estimated_bytes,
		..Default::default()
	}
}

fn violation_message(violation: &Violation) -> proto::GifLimitViolation {
	proto::GifLimitViolation {
		limit: match violation.limit {
			Limit::Duration => proto::GifLimit::Duration,
			Limit::Width => proto::GifLimit::Width,
			Limit::Height => proto::GifLimit::Height,
			Limit::Size => proto::GifLimit::Size,
		}
		.into(),
		message: violation.message.clone(),
		suggested_duration_ms: violation
			.suggested_duration
			.and_then(|d| i64::try_from(d.as_millis()).ok())
			.unwrap_or_default(),
		suggested_width: violation.suggested_width.unwrap_or_default(),
		..Default::default()
	}
}
