//! `CropService` and the crop operation of `JobsService`.
//!
//! The crop runs as a job (`SubmitJob` with a `CropOperation`); this module
//! turns that operation into a [`JobSpec`]. The logic is
//! `muxmultitool_core::crop`: the server resolves paths, probes the input,
//! asks the capability cache for the bitstream filter, reads the header's
//! current crop window with one decoded frame, and maps the core types to
//! the messages.
//!
//! `PlanCrop` runs the same checks as a submit, without queueing anything,
//! and answers a refusal (another codec, a build without the filter) as a
//! `CropSupport` rather than an error, so the frontend can disable the
//! option with the reason as a tooltip. `DetectBlackBars` runs ffmpeg's
//! `cropdetect` over a dozen keyframes and suggests the crop.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::crop::{self, Coded, Plan, Source, Unsupported, command, detect};
use muxmultitool_core::ffmpeg::command::ShellLine;
use muxmultitool_core::ffmpeg::{Capabilities, FfmpegCommand, Limits, ProbeResult, Tool};
use muxmultitool_core::jobs::JobSpec;
use muxmultitool_proto::connect::muxmultitool::crop::v1::CropService;
use muxmultitool_proto::proto::muxmultitool::crop::v1::{
	self as proto, CropOperation, DetectBlackBarsRequest, DetectBlackBarsResponse, PlanCropRequest,
	PlanCropResponse,
};
use tokio::sync::mpsc;

use crate::AppState;
use crate::jobs::{Jobs, PROBE_TIMEOUT, output_near};

/// How long black-bar detection may take. A dozen keyframes decode in a
/// second or two; the margin is for a slow network share.
const DETECT_TIMEOUT: Duration = Duration::from_secs(120);

/// Implements the crop service. It reuses the jobs service's path checks
/// and spec builders, so a plan and a submit go through the same code.
#[derive(Debug)]
pub(crate) struct Crop {
	jobs: Jobs,
}

impl Crop {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}
}

impl CropService for Crop {
	async fn plan_crop(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, PlanCropRequest>,
	) -> ServiceResult<PlanCropResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation.into_option() else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let input = self.jobs.cut_input(&operation.input)?;
		// The output is only shown in the command here; it need not exist.
		let output = output_near(&input, &operation.output, default_output);
		let source = match self.jobs.crop_source(&input).await? {
			Ok(source) => source,
			Err(unsupported) => {
				return Ok(Response::new(PlanCropResponse {
					support: Some(proto::CropSupport {
						supported: false,
						reason: unsupported.to_string(),
						..Default::default()
					})
					.into(),
					..Default::default()
				}));
			}
		};
		let plan = plan_of(&source, &operation)?;
		let coded = self.jobs.coded_picture(&input, &source).await?;
		let argv = self
			.jobs
			.state
			.runner
			.ffmpeg_invocation(&command::crop(&input, &output, &plan, &coded));
		Ok(Response::new(PlanCropResponse {
			support: Some(proto::CropSupport {
				supported: true,
				..Default::default()
			})
			.into(),
			source: Some(source_message(&source)).into(),
			crop: Some(window_message(plan.crop)).into(),
			width: plan.width,
			height: plan.height,
			command: ShellLine(&argv).to_string(),
			limitations: crop::limitations(source.codec),
			..Default::default()
		}))
	}

	async fn detect_black_bars(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, DetectBlackBarsRequest>,
	) -> ServiceResult<DetectBlackBarsResponse> {
		let request = request.to_owned_message();
		let input = self.jobs.cut_input(&request.input)?;
		let position = u64::try_from(request.position_ms)
			.map(Duration::from_millis)
			.map_err(|_| ConnectError::invalid_argument("position_ms must not be negative"))?;
		let source = self
			.jobs
			.crop_source(&input)
			.await?
			.map_err(|unsupported| ConnectError::failed_precondition(unsupported.to_string()))?;
		let lines = self
			.jobs
			.ffmpeg_log(&command::detect(&input, &source, position))
			.await?;
		let Some(found) = detect::parse_cropdetect(lines.iter().map(String::as_str), &source)
		else {
			return Err(ConnectError::failed_precondition(format!(
				"no picture edges were found in the keyframes from {} s on; pick a brighter position",
				position.as_secs()
			)));
		};
		let plan = crop::plan(&source, found.crop)
			.map_err(|error| ConnectError::failed_precondition(error.to_string()))?;
		Ok(Response::new(DetectBlackBarsResponse {
			crop: Some(window_message(plan.crop)).into(),
			frames_scanned: found.frames,
			width: plan.width,
			height: plan.height,
			..Default::default()
		}))
	}
}

impl Jobs {
	/// Validates a crop request and turns it into a job spec. The input
	/// must be H.264 or H.265 and the build must have its bitstream filter
	/// (`FAILED_PRECONDITION` with the reason otherwise), and the snapped
	/// crop must cut something and leave a picture (`INVALID_ARGUMENT`).
	pub(crate) async fn crop_spec(
		&self,
		name: &str,
		operation: &CropOperation,
	) -> Result<JobSpec, ConnectError> {
		let input = self.cut_input(&operation.input)?;
		let output = self.checked_output(
			&output_near(&input, &operation.output, default_output).to_string_lossy(),
		)?;
		let source = self
			.crop_source(&input)
			.await?
			.map_err(|unsupported| ConnectError::failed_precondition(unsupported.to_string()))?;
		let plan = plan_of(&source, operation)?;
		if plan.is_empty() {
			return Err(ConnectError::invalid_argument(
				"nothing to crop: every side is 0 after snapping to the crop unit",
			));
		}
		let coded = self.coded_picture(&input, &source).await?;
		Ok(JobSpec {
			name: job_name(name, &output),
			command: command::crop(&input, &output, &plan, &coded),
			expected_duration: source.duration,
			preparation: Vec::new(),
			scratch_dir: None,
		})
	}

	/// Probes `input`, picks the video stream and checks the build. The
	/// outer error is a request problem (not a media file, ffmpeg broken);
	/// the inner one is why this file cannot be cropped losslessly.
	async fn crop_source(&self, input: &Path) -> Result<Result<Source, Unsupported>, ConnectError> {
		let probe = self.probe_media(input).await?;
		let source = match Source::from_probe(&probe) {
			Ok(source) => source,
			Err(unsupported) => return Ok(Err(unsupported)),
		};
		let caps = self.build_capabilities().await?;
		Ok(source.check_build(&caps).map(|()| source))
	}

	/// The coded size and header crop window of `source` in `input`, from
	/// one decoded frame.
	async fn coded_picture(&self, input: &Path, source: &Source) -> Result<Coded, ConnectError> {
		let args = detect::coded_probe_args(input, source.stream_index);
		let args: Vec<&str> = args.iter().map(String::as_str).collect();
		let csv = self
			.state
			.runner
			.capture(Tool::Ffprobe, &args, &Limits::timeout(PROBE_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::failed_precondition(format!(
					"cannot read the coded picture size of the video: {error}"
				))
			})?;
		detect::parse_coded(&csv).ok_or_else(|| {
			ConnectError::failed_precondition(
				"ffprobe did not report the coded picture size of the video",
			)
		})
	}

	/// Runs `cmd` to completion and returns every line it wrote to stderr.
	async fn ffmpeg_log(&self, cmd: &FfmpegCommand) -> Result<Vec<String>, ConnectError> {
		let (tx, mut rx) = mpsc::channel(256);
		let collect = async {
			let mut lines = Vec::new();
			while let Some(line) = rx.recv().await {
				lines.push(line);
			}
			lines
		};
		let limits = Limits::timeout(DETECT_TIMEOUT);
		let (run, lines) = tokio::join!(
			self.state.runner.run_with_log(cmd, &limits, None, Some(tx)),
			collect
		);
		run.map_err(|error| {
			ConnectError::failed_precondition(format!("black-bar detection failed: {error}"))
		})?;
		Ok(lines)
	}

	async fn probe_media(&self, input: &Path) -> Result<ProbeResult, ConnectError> {
		self.state
			.runner
			.probe(input, &Limits::timeout(PROBE_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::invalid_argument(format!("the input is not a media file: {error}"))
			})
	}

	async fn build_capabilities(&self) -> Result<Arc<Capabilities>, ConnectError> {
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

/// The operation's crop snapped against `source`.
fn plan_of(source: &Source, operation: &CropOperation) -> Result<Plan, ConnectError> {
	let requested = operation
		.crop
		.as_option()
		.map(|window| crop::Crop {
			top: window.top,
			bottom: window.bottom,
			left: window.left,
			right: window.right,
		})
		.unwrap_or_default();
	crop::plan(source, requested).map_err(|error| ConnectError::invalid_argument(error.to_string()))
}

/// `movie.cropped.mkv` next to `movie.mkv`: the sketch's default template.
fn default_output(input: &Path) -> PathBuf {
	let stem = input
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_default();
	let name = match input.extension() {
		Some(ext) => format!("{stem}.cropped.{}", ext.to_string_lossy()),
		None => format!("{stem}.cropped"),
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

fn source_message(source: &Source) -> proto::CropSource {
	proto::CropSource {
		stream_index: source.stream_index,
		codec: source.codec.name().to_owned(),
		width: source.width,
		height: source.height,
		unit_x: source.unit.x,
		unit_y: source.unit.y,
		pix_fmt: source.pix_fmt.clone(),
		..Default::default()
	}
}

fn window_message(crop: crop::Crop) -> proto::CropWindow {
	proto::CropWindow {
		top: crop.top,
		bottom: crop.bottom,
		left: crop.left,
		right: crop.right,
		..Default::default()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_default_output_keeps_the_container() {
		assert_eq!(
			default_output(Path::new("/media/in/Movie A (2024).mkv")),
			PathBuf::from("/media/in/Movie A (2024).cropped.mkv")
		);
		assert_eq!(
			default_output(Path::new("/media/in/clip")),
			PathBuf::from("/media/in/clip.cropped")
		);
	}
}
