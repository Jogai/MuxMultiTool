//! `ExtractService` and the extract operation of `JobsService`.
//!
//! Writing every track (or a chosen subset) to its own file runs as one job
//! (`SubmitJob` with an `ExtractStreamsOperation`): one ffmpeg run with one
//! output per stream. The logic is `muxmultitool_core::extract`; this
//! module resolves the input and the output folder, probes the input and
//! maps the plan to the messages. `PreviewExtract` runs the same resolution
//! without queueing anything.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::extract::{self, ExtractError, Plan};
use muxmultitool_core::ffmpeg::Limits;
use muxmultitool_core::ffmpeg::command::ShellLine;
use muxmultitool_core::jobs::JobSpec;
use muxmultitool_core::streams::Selection;
use muxmultitool_proto::connect::muxmultitool::extract::v1::ExtractService;
use muxmultitool_proto::proto::muxmultitool::extract::v1::{
	self as proto, ExtractStreamsOperation, PreviewExtractRequest, PreviewExtractResponse,
};
use muxmultitool_proto::proto::muxmultitool::streams::v1::SelectionMode;

use crate::AppState;
use crate::jobs::{Jobs, PROBE_TIMEOUT};

/// Implements the extract service. It reuses the jobs service's path checks
/// and plan builder, so a preview and a submit go through the same code.
#[derive(Debug)]
pub(crate) struct Extract {
	jobs: Jobs,
}

impl Extract {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}
}

impl ExtractService for Extract {
	async fn preview_extract(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, PreviewExtractRequest>,
	) -> ServiceResult<PreviewExtractResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation.into_option() else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let resolved = self.jobs.extract_plan(&operation).await?;
		let argv = self
			.jobs
			.state
			.runner
			.ffmpeg_invocation(&resolved.plan.command(&resolved.input, &resolved.folder));
		Ok(Response::new(PreviewExtractResponse {
			files: resolved
				.plan
				.items
				.iter()
				.map(|item| proto::ExtractFile {
					stream_index: item.index,
					kind: item.kind.name().to_owned(),
					codec: item.codec.clone(),
					language: item.language.clone().unwrap_or_default(),
					file_name: item.file_name.clone(),
					..Default::default()
				})
				.collect(),
			skipped: resolved
				.plan
				.skipped
				.iter()
				.map(|skipped| proto::ExtractSkipped {
					stream_index: skipped.index,
					blocking: skipped.blocking,
					message: skipped.message.clone(),
					..Default::default()
				})
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

/// An operation resolved against its input.
struct Resolved {
	input: PathBuf,
	folder: PathBuf,
	duration: Option<Duration>,
	plan: Plan,
}

impl Jobs {
	/// Validates an extract request and turns it into a job spec. Every
	/// output must be new and no ticked stream may lack a file format;
	/// otherwise the submit fails with `ALREADY_EXISTS` or
	/// `FAILED_PRECONDITION`.
	pub(crate) async fn extract_streams_spec(
		&self,
		name: &str,
		operation: &ExtractStreamsOperation,
	) -> Result<JobSpec, ConnectError> {
		let resolved = self.extract_plan(operation).await?;
		if !resolved.plan.can_run() {
			return Err(ConnectError::failed_precondition(format!(
				"some selected streams cannot be written to a file: {}",
				resolved.plan.blocking_messages().join("; ")
			)));
		}
		// The folder is inside a media root already; no file may exist.
		for item in &resolved.plan.items {
			self.checked_output(&resolved.folder.join(&item.file_name).to_string_lossy())?;
		}
		Ok(JobSpec {
			name: if name.is_empty() {
				format!("Extract {} streams", resolved.plan.items.len())
			} else {
				name.to_owned()
			},
			command: resolved.plan.command(&resolved.input, &resolved.folder),
			expected_duration: resolved.duration,
		})
	}

	/// Resolves the input and the folder, probes the input and plans the
	/// selection. Existing outputs are not checked here: a preview may
	/// name files that exist, a submit may not.
	async fn extract_plan(
		&self,
		operation: &ExtractStreamsOperation,
	) -> Result<Resolved, ConnectError> {
		let input = self.stream_input(&operation.input).await?;
		let folder = if operation.output_folder.is_empty() {
			input
				.parent()
				.map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
		} else {
			// Borrow the output check for the folder: it must exist and lie
			// inside a media root.
			let probe_name = Path::new(&operation.output_folder).join("probe");
			let checked = self.checked_output(&probe_name.to_string_lossy())?;
			checked
				.parent()
				.map(Path::to_path_buf)
				.ok_or_else(|| ConnectError::invalid_argument("output_folder is not a folder"))?
		};
		let streams = operation.stream_indexes.iter().copied();
		let selection = match operation.mode.as_known() {
			Some(SelectionMode::Unspecified | SelectionMode::All) => Selection::all(),
			Some(SelectionMode::Keep) => Selection::keep(streams),
			Some(SelectionMode::Drop) => Selection::drop(streams),
			Some(SelectionMode::VideoOnly) => Selection::video_only(),
			Some(SelectionMode::Rules) => {
				return Err(ConnectError::invalid_argument(
					"extraction takes no stream rules; use ALL, KEEP, DROP or VIDEO_ONLY",
				));
			}
			None => {
				return Err(ConnectError::invalid_argument(format!(
					"unknown selection mode {}",
					operation.mode.to_i32()
				)));
			}
		};
		let probe = self
			.state
			.runner
			.probe(&input, &Limits::timeout(PROBE_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::invalid_argument(format!("the input is not a media file: {error}"))
			})?;
		let stem = input
			.file_stem()
			.map(|s| s.to_string_lossy().into_owned())
			.unwrap_or_default();
		let plan = extract::plan(&probe, &selection, &stem).map_err(|error| match error {
			ExtractError::NothingToExtract => ConnectError::failed_precondition(error.to_string()),
			ExtractError::Selection(_) => ConnectError::invalid_argument(error.to_string()),
		})?;
		Ok(Resolved {
			input,
			folder,
			duration: probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
			plan,
		})
	}
}
