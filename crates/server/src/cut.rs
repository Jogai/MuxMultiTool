//! `CutService` and the cut operation of `JobsService`.
//!
//! The logic is `muxmultitool_core::cut`: this module resolves the input,
//! probes it, fetches the keyframes the plan needs around each cut, checks
//! the outputs against the media roots, and maps the core types to the
//! messages. `PlanCut` and a submitted `CutOperation` go through the same
//! [`Jobs::cut_plan`], so what the frontend previews is what runs.
//!
//! Keyframes are read in a window around each cut ([`KEYFRAME_WINDOW`]);
//! a GOP longer than that makes the plan fail once, after which the whole
//! file is read. A smart cut ([`smart`]) reads the packets around its cuts
//! the same way, checks the file first ([`Cut::check_smart_cut`]), and
//! runs as a job of two commands with a scratch folder next to the
//! output.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::cut::{
	self, CutMode, Export, Plan, PlanError, Request, Segment, SegmentMode, Source, smart, timecode,
};
use muxmultitool_core::ffmpeg::command::ShellLine;
use muxmultitool_core::ffmpeg::probe::{CodecType, Keyframe, Packet};
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, ProbeResult};
use muxmultitool_core::files::FileId;
use muxmultitool_core::jobs::JobSpec;
use muxmultitool_proto::connect::muxmultitool::cut::v1::CutService;
use muxmultitool_proto::proto::muxmultitool::cut::v1::{
	self as proto, CheckSmartCutRequest, CheckSmartCutResponse, CutExport, CutOperation,
	GetKeyframesRequest, GetKeyframesResponse, GetProjectRequest, GetProjectResponse,
	ParseTimeRequest, ParseTimeResponse, PlanCutRequest, PlanCutResponse, RedoProjectRequest,
	RedoProjectResponse, UndoProjectRequest, UndoProjectResponse, UpdateProjectRequest,
	UpdateProjectResponse,
};

use crate::AppState;
use crate::jobs::{Jobs, PROBE_TIMEOUT};

/// How far around a cut the keyframes are read, in seconds. Longer GOPs
/// are rare; they cost a second read of the whole file.
pub(crate) const KEYFRAME_WINDOW: f64 = 30.0;
/// How long a keyframe listing may take. Reading the whole file is the
/// slow case.
const KEYFRAME_TIMEOUT: Duration = Duration::from_secs(600);

/// Implements the cut service on the jobs service's path checks.
#[derive(Debug)]
pub(crate) struct Cut {
	jobs: Jobs,
}

impl Cut {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}
}

impl CutService for Cut {
	async fn get_keyframes(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, GetKeyframesRequest>,
	) -> ServiceResult<GetKeyframesResponse> {
		let request = request.to_owned_message();
		let input = self.jobs.cut_input(&request.file_id)?;
		let (_, source) = self.jobs.cut_source(&input).await?;
		let from = finite(request.from_seconds, "from_seconds")?.max(0.0);
		let to = if request.to_seconds <= 0.0 {
			source.duration
		} else {
			finite(request.to_seconds, "to_seconds")?.min(source.duration)
		};
		let mut response = GetKeyframesResponse {
			duration_seconds: source.duration,
			..Default::default()
		};
		let Some(stream) = source.video_stream else {
			return Ok(Response::new(response));
		};
		response.stream_index = stream;
		if to > from {
			let window = [(source.start_time + from, source.start_time + to)];
			let keyframes = self.jobs.keyframes(&input, stream, &window).await?;
			response.keyframes_seconds = keyframes
				.iter()
				.map(|k| k.pts - source.start_time)
				.filter(|t| *t <= to + cut::SNAP_TOLERANCE)
				.collect();
		}
		Ok(Response::new(response))
	}

	async fn parse_time(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, ParseTimeRequest>,
	) -> ServiceResult<ParseTimeResponse> {
		let request = request.to_owned_message();
		let (fps, duration) = if request.file_id.is_empty() {
			(None, None)
		} else {
			let input = self.jobs.cut_input(&request.file_id)?;
			let (probe, source) = self.jobs.cut_source(&input).await?;
			let fps = probe
				.streams_of(CodecType::Video)
				.find(|s| Some(s.index) == source.video_stream)
				.and_then(muxmultitool_core::ffmpeg::probe::Stream::frame_rate);
			(fps, Duration::try_from_secs_f64(source.duration).ok())
		};
		let time = timecode::parse_with_fps(&request.text, fps)
			.map_err(|error| ConnectError::invalid_argument(error.to_string()))?;
		if let Some(duration) = duration {
			timecode::within(time, duration)
				.map_err(|error| ConnectError::invalid_argument(error.to_string()))?;
		}
		Ok(Response::new(ParseTimeResponse {
			seconds: time.as_secs_f64(),
			timecode: timecode::format(time),
			..Default::default()
		}))
	}

	async fn plan_cut(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, PlanCutRequest>,
	) -> ServiceResult<PlanCutResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation.into_option() else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let planned = self.jobs.cut_plan(&operation).await?;
		let lines: Vec<String> = planned
			.commands()
			.iter()
			.map(|command| {
				ShellLine(&self.jobs.state.runner.ffmpeg_invocation(command)).to_string()
			})
			.collect();
		let mut response = PlanCutResponse {
			command: lines.join(" && "),
			commands: if lines.len() > 1 { lines } else { Vec::new() },
			..Default::default()
		};
		let Planned { input, plan, smart } = planned;
		if let Some(smart) = smart {
			response.segments = smart.ranges.iter().map(range_message).collect();
			response.outputs = paths(&smart.outputs());
			response.total_duration_seconds = smart.ranges.iter().map(smart::Range::length).sum();
		} else {
			response.segments = plan.segments.iter().map(segment_message).collect();
			response.outputs = paths(&plan.outputs());
			response.total_duration_seconds = plan.total_length();
			response.concat_list = plan.concat_list(&input).unwrap_or_default();
		}
		Ok(Response::new(response))
	}

	async fn check_smart_cut(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, CheckSmartCutRequest>,
	) -> ServiceResult<CheckSmartCutResponse> {
		let request = request.to_owned_message();
		let input = self.jobs.cut_input(&request.file_id)?;
		let (probe, source) = self.jobs.cut_source(&input).await?;
		let video_codec = source
			.video_stream
			.and_then(|index| probe.streams.iter().find(|s| s.index == index))
			.and_then(|s| s.codec_name.clone())
			.unwrap_or_default();
		let response = match self.jobs.smart_check(&probe, &source).await? {
			Ok(video) => CheckSmartCutResponse {
				supported: true,
				video_codec,
				encoder: video.codec.encoder().to_owned(),
				..Default::default()
			},
			Err(reason) => CheckSmartCutResponse {
				supported: false,
				reason: reason.to_string(),
				video_codec,
				..Default::default()
			},
		};
		Ok(Response::new(response))
	}

	// The project methods live in `crate::project`.

	async fn get_project(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, GetProjectRequest>,
	) -> ServiceResult<GetProjectResponse> {
		let request = request.to_owned_message();
		let project = crate::project::get(&self.jobs, &request.file_id).await?;
		Ok(Response::new(GetProjectResponse {
			project: Some(project).into(),
			..Default::default()
		}))
	}

	async fn update_project(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, UpdateProjectRequest>,
	) -> ServiceResult<UpdateProjectResponse> {
		let request = request.to_owned_message();
		let state = request.state.into_option();
		let project = crate::project::update(&self.jobs, &request.file_id, state).await?;
		Ok(Response::new(UpdateProjectResponse {
			project: Some(project).into(),
			..Default::default()
		}))
	}

	async fn undo_project(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, UndoProjectRequest>,
	) -> ServiceResult<UndoProjectResponse> {
		let request = request.to_owned_message();
		let project = crate::project::undo(&self.jobs, &request.file_id).await?;
		Ok(Response::new(UndoProjectResponse {
			project: Some(project).into(),
			..Default::default()
		}))
	}

	async fn redo_project(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, RedoProjectRequest>,
	) -> ServiceResult<RedoProjectResponse> {
		let request = request.to_owned_message();
		let project = crate::project::redo(&self.jobs, &request.file_id).await?;
		Ok(Response::new(RedoProjectResponse {
			project: Some(project).into(),
			..Default::default()
		}))
	}
}

/// A cut request resolved against its input: the base plan, and the
/// pieces of a smart cut on top of it.
struct Planned {
	input: PathBuf,
	plan: Plan,
	smart: Option<smart::Plan>,
}

impl Planned {
	/// The commands the job runs, in order.
	fn commands(&self) -> Vec<FfmpegCommand> {
		match &self.smart {
			Some(smart) => smart.commands(&self.input),
			None => vec![self.plan.command(&self.input)],
		}
	}
}

impl Jobs {
	/// Validates a cut request and turns it into a job spec; see
	/// [`Jobs::cut_plan`].
	pub(crate) async fn cut_spec(
		&self,
		name: &str,
		operation: &CutOperation,
	) -> Result<JobSpec, ConnectError> {
		let planned = self.cut_plan(operation).await?;
		let mut commands = planned.commands();
		let command = commands.pop().unwrap_or_default();
		let Planned { plan, smart, .. } = planned;
		let name = if name.is_empty() {
			plan.outputs()
				.first()
				.and_then(|p| p.file_name())
				.map(|n| n.to_string_lossy().into_owned())
				.unwrap_or_default()
		} else {
			name.to_owned()
		};
		let scratch_dir = match smart {
			Some(smart) => {
				tokio::fs::create_dir_all(&smart.scratch_dir)
					.await
					.map_err(|error| {
						ConnectError::internal(format!(
							"cannot create the scratch folder {}: {error}",
							smart.scratch_dir.display()
						))
					})?;
				Some(smart.scratch_dir)
			}
			None => None,
		};
		Ok(JobSpec {
			name,
			command,
			expected_duration: plan.expected_duration(),
			preparation: commands,
			scratch_dir,
		})
	}

	/// Resolves, probes and plans `operation`: the input must be a media
	/// file inside the roots, the ranges must leave something to export,
	/// the keyframes the modes need must exist, and every output must be a
	/// new file in an existing folder inside the roots.
	async fn cut_plan(&self, operation: &CutOperation) -> Result<Planned, ConnectError> {
		let input = self.cut_input(&operation.input)?;
		let (probe, source) = self.cut_source(&input).await?;
		let request = cut_request(operation)?;
		let windows = cut::keyframe_windows(&request, &source, KEYFRAME_WINDOW);
		let stream = source.video_stream.unwrap_or_default();
		let keyframes = if windows.is_empty() {
			Vec::new()
		} else {
			self.keyframes(&input, stream, &windows).await?
		};
		let plan = match cut::plan(&request, &source, &keyframes) {
			Err(PlanError::NoKeyframeBefore { .. } | PlanError::NoKeyframeInside { .. })
				if !windows.is_empty() =>
			{
				tracing::info!(input = %input.display(), "no keyframe near a cut; reading the whole file");
				let all = self
					.keyframes(&input, stream, &cut::whole_file_window(&source))
					.await?;
				cut::plan(&request, &source, &all)
			}
			other => other,
		}
		.map_err(|error| plan_error(&error))?;
		for output in plan.outputs() {
			self.checked_output(&output.to_string_lossy())?;
		}
		let smart = if plan.cut_mode == CutMode::Smart {
			Some(self.smart_plan(&input, &probe, &source, &plan).await?)
		} else {
			None
		};
		Ok(Planned { input, plan, smart })
	}

	/// Whether `probe` can be smart cut with the installed ffmpeg; the
	/// outer error is ffmpeg not answering at all.
	async fn smart_check(
		&self,
		probe: &ProbeResult,
		source: &Source,
	) -> Result<Result<smart::Video, smart::Unsupported>, ConnectError> {
		let capabilities = self
			.state
			.capabilities
			.get(&self.state.runner)
			.await
			.map_err(|error| {
				ConnectError::failed_precondition(format!("ffmpeg is not available: {error}"))
			})?;
		Ok(smart::check(probe, source, |name| {
			capabilities.has_encoder(name)
		}))
	}

	/// The pieces of a smart cut: the file must be supported, and the
	/// packets around the cuts are read, the whole ranges when the window
	/// does not reach their keyframes.
	async fn smart_plan(
		&self,
		input: &Path,
		probe: &ProbeResult,
		source: &Source,
		base: &Plan,
	) -> Result<smart::Plan, ConnectError> {
		let video = self
			.smart_check(probe, source)
			.await?
			.map_err(|error| ConnectError::failed_precondition(error.to_string()))?;
		let streams = smart::copied_streams(probe);
		let scratch = scratch_dir(base);
		let windows = smart::windows(base, source, KEYFRAME_WINDOW);
		let packets = self.packets(input, video.stream, &windows).await?;
		let plan = smart::plan(
			base,
			source,
			video.clone(),
			streams.clone(),
			&packets,
			Some(KEYFRAME_WINDOW),
			&scratch,
		);
		match plan {
			Err(smart::PlanError::ReadWholeRange { .. }) => {
				tracing::info!(input = %input.display(), "no keyframe near a cut; reading the whole range");
				let all = self
					.packets(
						input,
						video.stream,
						&smart::whole_range_windows(base, source),
					)
					.await?;
				smart::plan(base, source, video, streams, &all, None, &scratch)
			}
			other => other,
		}
		.map_err(|error| match error {
			smart::PlanError::Unsupported(_) => {
				ConnectError::failed_precondition(error.to_string())
			}
			smart::PlanError::NoFrame { .. } => ConnectError::invalid_argument(error.to_string()),
			smart::PlanError::ReadWholeRange { .. } => ConnectError::internal(error.to_string()),
		})
	}

	/// The packets of `stream` of `input` within `windows`.
	async fn packets(
		&self,
		input: &Path,
		stream: u32,
		windows: &[(f64, f64)],
	) -> Result<Vec<Packet>, ConnectError> {
		self.state
			.runner
			.packets_in(input, stream, windows, &Limits::timeout(KEYFRAME_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::failed_precondition(format!(
					"could not read the frames of {}: {error}",
					input.display()
				))
			})
	}

	/// The path behind a file id from `FilesService`, or an absolute path
	/// inside a media root; either way an existing file.
	pub(crate) fn cut_input(&self, id_or_path: &str) -> Result<PathBuf, ConnectError> {
		if id_or_path.is_empty() {
			return Err(ConnectError::invalid_argument("input is required"));
		}
		if let Ok(path) = self.state.files.path(&FileId::new(id_or_path)) {
			if !path.is_file() {
				return Err(ConnectError::invalid_argument(format!(
					"input is not a file: {}",
					path.display()
				)));
			}
			return Ok(path);
		}
		self.checked_input(id_or_path)
	}

	/// Probes `input` and reads what a cut needs from the report.
	pub(crate) async fn cut_source(
		&self,
		input: &Path,
	) -> Result<(ProbeResult, Source), ConnectError> {
		let probe = self
			.state
			.runner
			.probe(input, &Limits::timeout(PROBE_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::invalid_argument(format!("the input is not a media file: {error}"))
			})?;
		let source = Source::from_probe(&probe, input)
			.map_err(|error| ConnectError::failed_precondition(error.to_string()))?;
		Ok((probe, source))
	}

	/// The keyframes of `stream` of `input` within `windows`.
	pub(crate) async fn keyframes(
		&self,
		input: &Path,
		stream: u32,
		windows: &[(f64, f64)],
	) -> Result<Vec<Keyframe>, ConnectError> {
		self.state
			.runner
			.keyframes_in(input, stream, windows, &Limits::timeout(KEYFRAME_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::failed_precondition(format!(
					"could not read the keyframes of {}: {error}",
					input.display()
				))
			})
	}
}

/// The core request of an operation message.
fn cut_request(operation: &CutOperation) -> Result<Request, ConnectError> {
	let segment_mode = match operation.segment_mode.as_known() {
		Some(proto::SegmentMode::Unspecified | proto::SegmentMode::Keep) => SegmentMode::Keep,
		Some(proto::SegmentMode::Remove) => SegmentMode::Remove,
		None => {
			return Err(ConnectError::invalid_argument(format!(
				"unknown segment mode {}",
				operation.segment_mode.to_i32()
			)));
		}
	};
	let cut_mode = match operation.cut_mode.as_known() {
		Some(proto::CutMode::Unspecified | proto::CutMode::Keyframe) => CutMode::Keyframe,
		Some(proto::CutMode::Normal) => CutMode::Normal,
		Some(proto::CutMode::Smart) => CutMode::Smart,
		None => {
			return Err(ConnectError::invalid_argument(format!(
				"unknown cut mode {}",
				operation.cut_mode.to_i32()
			)));
		}
	};
	let export = match operation.export.as_known() {
		Some(CutExport::Unspecified | CutExport::SeparateFiles) => Export::SeparateFiles,
		Some(CutExport::Merged) => Export::Merged,
		None => {
			return Err(ConnectError::invalid_argument(format!(
				"unknown export {}",
				operation.export.to_i32()
			)));
		}
	};
	if operation.output.is_empty() {
		return Err(ConnectError::invalid_argument("output path is required"));
	}
	let segments = operation
		.segments
		.iter()
		.map(|s| {
			Ok(Segment::new(
				finite(s.start_seconds, "start_seconds")?,
				finite(s.end_seconds, "end_seconds")?,
			))
		})
		.collect::<Result<Vec<_>, ConnectError>>()?;
	Ok(Request {
		segments,
		segment_mode,
		cut_mode,
		export,
		output: PathBuf::from(&operation.output),
	})
}

/// A finite, non-negative time field.
pub(crate) fn finite(value: f64, field: &str) -> Result<f64, ConnectError> {
	if value.is_finite() && value >= 0.0 {
		Ok(value)
	} else {
		Err(ConnectError::invalid_argument(format!(
			"{field} must be a non-negative number"
		)))
	}
}

fn plan_error(error: &PlanError) -> ConnectError {
	match error {
		PlanError::NoSegments | PlanError::DuplicateOutput { .. } => {
			ConnectError::invalid_argument(error.to_string())
		}
		PlanError::NoKeyframeBefore { .. }
		| PlanError::NoKeyframeInside { .. }
		| PlanError::MergedNeedsIndex { .. }
		| PlanError::MergedNeedsVideo => ConnectError::failed_precondition(error.to_string()),
	}
}

/// The folder for the pieces of a smart cut: a hidden one next to the
/// output, unique per plan, on the same file system as the output so the
/// pieces take no space elsewhere.
fn scratch_dir(plan: &Plan) -> PathBuf {
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_nanos();
	let name = format!(".muxmultitool-smartcut-{}-{nanos:x}", std::process::id());
	plan.outputs()
		.first()
		.and_then(|output| output.parent())
		.map_or_else(|| PathBuf::from(&name), |parent| parent.join(&name))
}

fn paths(paths: &[PathBuf]) -> Vec<String> {
	paths
		.iter()
		.map(|p| p.to_string_lossy().into_owned())
		.collect()
}

fn range_message(range: &smart::Range) -> proto::PlannedSegment {
	proto::PlannedSegment {
		number: u32::try_from(range.number).unwrap_or(u32::MAX),
		start_seconds: range.segment.start,
		end_seconds: range.segment.end,
		cut_start_seconds: range.segment.start,
		output: range.output.to_string_lossy().into_owned(),
		reencoded_seconds: range.encoded_seconds(),
		..Default::default()
	}
}

fn segment_message(segment: &cut::PlannedSegment) -> proto::PlannedSegment {
	proto::PlannedSegment {
		number: u32::try_from(segment.number).unwrap_or(u32::MAX),
		start_seconds: segment.segment.start,
		end_seconds: segment.segment.end,
		cut_start_seconds: segment.cut_start,
		output: segment.output.to_string_lossy().into_owned(),
		..Default::default()
	}
}
