//! `CombineService` and the combine operation of `JobsService`.
//!
//! A multi-source mux runs as a job (`SubmitJob` with a
//! `CombineOperation`); this module turns that operation into a
//! [`JobSpec`]. The logic is `muxmultitool_core::combine`: the server only
//! resolves the sources (file ids or paths) and the output, probes every
//! source, and maps the plan to the messages.
//!
//! `PreviewCombine` runs the same resolution as a submit without queueing
//! anything: it returns every source with its streams, every output track
//! with where it comes from and the metadata it will carry, the issues the
//! target container raises, the warnings, and the exact command, so the
//! frontend shows the result while the user adds tracks and edits them.

use std::path::PathBuf;
use std::sync::Arc;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::combine::{self, OutputTrack, Plan, PlanSource, Source, Track};
use muxmultitool_core::ffmpeg::command::ShellLine;
use muxmultitool_core::ffmpeg::{Limits, ProbeResult};
use muxmultitool_core::jobs::JobSpec;
use muxmultitool_core::report;
use muxmultitool_core::streams::Container;
use muxmultitool_proto::connect::muxmultitool::combine::v1::CombineService;
use muxmultitool_proto::proto::muxmultitool::combine::v1::{
	self as proto, CombineOperation, PreviewCombineRequest, PreviewCombineResponse,
};
use muxmultitool_proto::proto::muxmultitool::files::v1::StreamSummary;

use crate::AppState;
use crate::jobs::{Jobs, PROBE_TIMEOUT};
use crate::streams::{extensions, file_name, output_path};

/// Implements the combine service. It reuses the jobs service's path checks
/// and plan builder, so a preview and a submit go through the same code.
#[derive(Debug)]
pub(crate) struct Combine {
	jobs: Jobs,
}

impl Combine {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}
}

impl CombineService for Combine {
	async fn preview_combine(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, PreviewCombineRequest>,
	) -> ServiceResult<PreviewCombineResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation.into_option() else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let resolved = self.jobs.combine_plan(&operation).await?;
		let argv = self
			.jobs
			.state
			.runner
			.ffmpeg_invocation(&resolved.plan.command(&resolved.output));
		// The quick report per source, for the same stream rows the Files
		// screen shows.
		let reports: Vec<Vec<StreamSummary>> = resolved
			.probes
			.iter()
			.map(|probe| {
				report::build(probe, None)
					.streams
					.into_iter()
					.map(crate::report::stream)
					.collect()
			})
			.collect();
		let tracks = resolved
			.plan
			.tracks
			.iter()
			.map(|track| {
				let stream = reports[track.source]
					.iter()
					.find(|s| s.index == track.stream)
					.cloned();
				track_message(track, stream)
			})
			.collect();
		Ok(Response::new(PreviewCombineResponse {
			container: resolved.plan.container.name().to_owned(),
			output_name: file_name(&resolved.output),
			sources: resolved
				.plan
				.sources
				.iter()
				.zip(reports)
				.map(|(source, streams)| source_message(source, streams))
				.collect(),
			tracks,
			issues: resolved
				.plan
				.issues
				.iter()
				.map(|issue| proto::TrackIssue {
					track: u32::try_from(issue.track).unwrap_or(u32::MAX),
					blocking: issue.blocking,
					message: issue.message.clone(),
					..Default::default()
				})
				.collect(),
			warnings: resolved
				.plan
				.warnings
				.iter()
				.map(ToString::to_string)
				.collect(),
			can_run: resolved.plan.can_run(),
			command: ShellLine(&argv).to_string(),
			argv: argv
				.iter()
				.map(|a| a.to_string_lossy().into_owned())
				.collect(),
			..Default::default()
		}))
	}
}

/// An operation resolved against its sources.
struct Resolved {
	output: PathBuf,
	/// One probe per source, in operation order.
	probes: Vec<ProbeResult>,
	plan: Plan,
}

impl Jobs {
	/// Validates a combine request and turns it into a job spec. The output
	/// must be new, and the target container must be able to hold every
	/// track; otherwise the submit fails with `FAILED_PRECONDITION` and the
	/// issues' messages.
	pub(crate) async fn combine_spec(
		&self,
		name: &str,
		operation: &CombineOperation,
	) -> Result<JobSpec, ConnectError> {
		let resolved = self.combine_plan(operation).await?;
		let output = self.checked_output(&resolved.output.to_string_lossy())?;
		if !resolved.plan.can_run() {
			return Err(ConnectError::failed_precondition(format!(
				"the {} output cannot hold every track: {}",
				resolved.plan.container,
				resolved.plan.blocking_messages().join("; ")
			)));
		}
		Ok(JobSpec {
			name: if name.is_empty() {
				file_name(&output)
			} else {
				name.to_owned()
			},
			command: resolved.plan.command(&output),
			expected_duration: resolved.plan.expected_duration(),
			preparation: Vec::new(),
			scratch_dir: None,
			outputs: vec![output],
		})
	}

	/// Resolves the sources and the output, probes every source and plans
	/// the tracks. The output is not checked for existence here: a preview
	/// may name an output that exists, a submit may not.
	async fn combine_plan(&self, operation: &CombineOperation) -> Result<Resolved, ConnectError> {
		if operation.sources.is_empty() {
			return Err(ConnectError::invalid_argument(
				combine::PlanError::NoSources.to_string(),
			));
		}
		let mut sources = Vec::with_capacity(operation.sources.len());
		let mut probes = Vec::with_capacity(operation.sources.len());
		for (i, source) in operation.sources.iter().enumerate() {
			let path = self.stream_input(&source.input).await?;
			let probe = self
				.state
				.runner
				.probe(&path, &Limits::timeout(PROBE_TIMEOUT))
				.await
				.map_err(|error| {
					ConnectError::invalid_argument(format!(
						"source {} is not a media file: {error}",
						combine::label(i)
					))
				})?;
			sources.push(Source::from_probe(&path, &probe).with_offset(source.offset_seconds));
			probes.push(probe);
		}
		let output = output_path(&sources[0].path, &operation.output)?;
		let Some(container) = Container::from_extension(&output) else {
			return Err(ConnectError::invalid_argument(format!(
				"the output's extension picks the container; use one of {}",
				extensions()
			)));
		};
		let tracks: Vec<Track> = operation
			.tracks
			.iter()
			.map(|track| Track {
				source: track.source as usize,
				stream: track.stream_index,
				language: track.language.clone(),
				title: track.title.clone(),
				default: track.default,
				forced: track.forced,
			})
			.collect();
		let plan = combine::plan(&sources, &tracks, container)
			.map_err(|error| ConnectError::invalid_argument(error.to_string()))?;
		Ok(Resolved {
			output,
			probes,
			plan,
		})
	}
}

fn source_message(source: &PlanSource, streams: Vec<StreamSummary>) -> proto::SourceInfo {
	proto::SourceInfo {
		label: source.label.clone(),
		path: source.path.to_string_lossy().into_owned(),
		name: source.name.clone(),
		duration_ms: source
			.duration
			.and_then(|d| i64::try_from(d.as_millis()).ok())
			.unwrap_or_default(),
		offset_seconds: source.offset_secs,
		used: source.used(),
		streams,
		..Default::default()
	}
}

fn track_message(track: &OutputTrack, stream: Option<StreamSummary>) -> proto::OutputTrack {
	proto::OutputTrack {
		position: u32::try_from(track.position).unwrap_or(u32::MAX),
		source: u32::try_from(track.source).unwrap_or(u32::MAX),
		origin: track.origin(),
		stream: stream.into(),
		language: track.language.clone().unwrap_or_default(),
		title: track.title.clone().unwrap_or_default(),
		default: track.default,
		forced: track.forced,
		edited: !track.edits.is_empty(),
		..Default::default()
	}
}
