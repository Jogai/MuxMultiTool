//! `StreamsService` and the stream-copy operation of `JobsService`.
//!
//! A remux or a selective stream copy runs as a job (`SubmitJob` with a
//! `SelectStreamsOperation`); this module turns that operation into a
//! [`JobSpec`]. The logic is `muxmultitool_core::streams`: the server only
//! resolves the input (a file id or a path) and the output, probes the
//! input, and maps the plan to the messages.
//!
//! `PreviewStreams` runs the same resolution as a submit without queueing
//! anything: it returns every stream of the input marked kept or dropped,
//! the issues the target container raises, and the exact command, so the
//! frontend shows the result while the user ticks streams.
//!
//! Mode `RULES` resolves `StreamRules` (`muxmultitool_core::streams::rules`)
//! instead of ticks: the "why" of every stream then names the rule that
//! decided it, and the preview carries the rules' warnings.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::ffmpeg::command::ShellLine;
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{Limits, ProbeResult};
use muxmultitool_core::files::{FileId, Kind};
use muxmultitool_core::jobs::JobSpec;
use muxmultitool_core::report;
use muxmultitool_core::streams::rules::{Matcher, Rule, Rules, Untagged};
use muxmultitool_core::streams::{self, Container, Plan, Selection};
use muxmultitool_proto::connect::muxmultitool::streams::v1::StreamsService;
use muxmultitool_proto::proto::muxmultitool::files::v1::StreamType;
use muxmultitool_proto::proto::muxmultitool::streams::v1::__buffa::oneof::stream_rule::Match;
use muxmultitool_proto::proto::muxmultitool::streams::v1::{
	self as proto, PreviewStreamsRequest, PreviewStreamsResponse, RuleAction,
	SelectStreamsOperation, SelectionMode, StreamRule, StreamRules, UntaggedLanguage,
};
use tracing::warn;

use crate::AppState;
use crate::files::connect_error;
use crate::jobs::{Jobs, PROBE_TIMEOUT};

/// Implements the streams service. It reuses the jobs service's path checks
/// and plan builder, so a preview and a submit go through the same code.
#[derive(Debug)]
pub(crate) struct Streams {
	jobs: Jobs,
}

impl Streams {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}
}

impl StreamsService for Streams {
	async fn preview_streams(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, PreviewStreamsRequest>,
	) -> ServiceResult<PreviewStreamsResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation.into_option() else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let resolved = self.jobs.stream_plan(&operation).await?;
		let argv = self
			.jobs
			.state
			.runner
			.ffmpeg_invocation(&resolved.plan.command(&resolved.input, &resolved.output));
		// The quick report, for the same stream rows the Files screen shows.
		let report = report::build(&resolved.probe, None);
		let streams = report
			.streams
			.into_iter()
			.zip(&resolved.plan.decisions)
			.zip(resolved.whys)
			.map(|((stream, decision), why)| proto::ResultStream {
				stream: Some(crate::report::stream(stream)).into(),
				kept: decision.kept,
				why,
				..Default::default()
			})
			.collect();
		Ok(Response::new(PreviewStreamsResponse {
			container: resolved.plan.container.name().to_owned(),
			output_name: file_name(&resolved.output),
			streams,
			issues: resolved
				.plan
				.issues
				.iter()
				.map(|issue| proto::StreamIssue {
					stream_index: issue.index,
					blocking: issue.blocking,
					message: issue.message.clone(),
					..Default::default()
				})
				.collect(),
			can_run: resolved.plan.can_run(),
			command: ShellLine(&argv).to_string(),
			warnings: resolved.warnings,
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
	output: PathBuf,
	probe: ProbeResult,
	plan: Plan,
	/// The "why" of every stream, in the plan's order: the plan's reason,
	/// or the rule that decided it.
	whys: Vec<String>,
	/// The rules' warnings; empty for the other modes.
	warnings: Vec<String>,
}

impl Jobs {
	/// Validates a stream-copy request and turns it into a job spec. The
	/// output must be new, and the target container must be able to hold
	/// every kept stream; otherwise the submit fails with
	/// `FAILED_PRECONDITION` and the issues' messages.
	pub(crate) async fn select_streams_spec(
		&self,
		name: &str,
		operation: &SelectStreamsOperation,
	) -> Result<JobSpec, ConnectError> {
		let resolved = self.stream_plan(operation).await?;
		let output = self.checked_output(&resolved.output.to_string_lossy())?;
		if !resolved.plan.can_run() {
			return Err(ConnectError::failed_precondition(format!(
				"the {} output cannot hold every selected stream: {}",
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
			command: resolved.plan.command(&resolved.input, &output),
			expected_duration: resolved
				.probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
		})
	}

	/// Resolves the input and output, probes the input and plans the
	/// selection. The output is not checked for existence here: a preview
	/// may name an output that exists, a submit may not.
	async fn stream_plan(
		&self,
		operation: &SelectStreamsOperation,
	) -> Result<Resolved, ConnectError> {
		let input = self.stream_input(&operation.input).await?;
		let output = output_path(&input, &operation.output)?;
		let Some(container) = Container::from_extension(&output) else {
			return Err(ConnectError::invalid_argument(format!(
				"the output's extension picks the container; use one of {}",
				extensions()
			)));
		};
		let streams = operation.stream_indexes.iter().copied();
		let selection = match operation.mode.as_known() {
			Some(SelectionMode::Unspecified | SelectionMode::All) => Selection::all(),
			Some(SelectionMode::Keep) => Selection::keep(streams),
			Some(SelectionMode::Drop) => Selection::drop(streams),
			Some(SelectionMode::VideoOnly) => Selection::video_only(),
			Some(SelectionMode::Rules) => {
				let rules = rules(operation.rules.as_option())?;
				let probe = self.probe_stream_input(&input).await?;
				let planned = streams::rules::plan(&probe, &rules, container)
					.map_err(|error| ConnectError::invalid_argument(error.to_string()))?;
				return Ok(Resolved {
					input,
					output,
					probe,
					plan: planned.plan,
					whys: planned.decisions.into_iter().map(|d| d.why).collect(),
					warnings: planned.warnings.iter().map(ToString::to_string).collect(),
				});
			}
			None => {
				return Err(ConnectError::invalid_argument(format!(
					"unknown selection mode {}",
					operation.mode.to_i32()
				)));
			}
		};
		let probe = self.probe_stream_input(&input).await?;
		let plan = streams::plan(&probe, &selection, container)
			.map_err(|error| ConnectError::invalid_argument(error.to_string()))?;
		let whys = plan
			.decisions
			.iter()
			.map(|d| d.reason.to_string())
			.collect();
		Ok(Resolved {
			input,
			output,
			probe,
			plan,
			whys,
			warnings: Vec::new(),
		})
	}

	async fn probe_stream_input(&self, input: &Path) -> Result<ProbeResult, ConnectError> {
		self.state
			.runner
			.probe(input, &Limits::timeout(PROBE_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::invalid_argument(format!("the input is not a media file: {error}"))
			})
	}

	/// The input as a path: an opaque id from `FilesService`, or the
	/// absolute path of a file inside a media root.
	pub(crate) async fn stream_input(&self, input: &str) -> Result<PathBuf, ConnectError> {
		let id = FileId::new(input);
		if id.decode().is_err() {
			return self.checked_input(input);
		}
		let state = Arc::clone(&self.state);
		let (entry, path) = tokio::task::spawn_blocking(move || {
			let entry = state.files.get(&id)?;
			let path = state.files.path(&id)?;
			Ok::<_, muxmultitool_core::files::Error>((entry, path))
		})
		.await
		.map_err(|error| {
			warn!(%error, "files task failed");
			ConnectError::internal("the files task failed")
		})?
		.map_err(connect_error)?;
		if entry.kind != Kind::File {
			return Err(ConnectError::invalid_argument(format!(
				"input is a folder, not a file: {}",
				entry.name
			)));
		}
		Ok(path)
	}
}

/// The rules of a `RULES` operation as the core sees them. Every rule
/// needs an action and a match; the core checks the values.
fn rules(rules: Option<&StreamRules>) -> Result<Rules, ConnectError> {
	let Some(rules) = rules else {
		return Err(ConnectError::invalid_argument(
			"mode RULES needs rules; use mode ALL for a remux",
		));
	};
	let untagged = match rules.untagged.as_known() {
		Some(UntaggedLanguage::Unspecified | UntaggedLanguage::Keep) => Untagged::Keep,
		Some(UntaggedLanguage::Drop) => Untagged::Drop,
		None => {
			return Err(ConnectError::invalid_argument(format!(
				"unknown untagged setting {}",
				rules.untagged.to_i32()
			)));
		}
	};
	let rules = rules
		.rules
		.iter()
		.enumerate()
		.map(|(i, r)| {
			rule(r).map_err(|what| ConnectError::invalid_argument(format!("rule {i} {what}")))
		})
		.collect::<Result<Vec<_>, _>>()?;
	Ok(Rules { rules, untagged })
}

/// One rule, or what is wrong with it.
fn rule(rule: &StreamRule) -> Result<Rule, String> {
	let stream_type = match rule.stream_type.as_known() {
		Some(StreamType::Unspecified) => None,
		Some(StreamType::Video) => Some(CodecType::Video),
		Some(StreamType::Audio) => Some(CodecType::Audio),
		Some(StreamType::Subtitle) => Some(CodecType::Subtitle),
		Some(StreamType::Data) => Some(CodecType::Data),
		Some(StreamType::Attachment) => Some(CodecType::Attachment),
		None => {
			return Err(format!(
				"has an unknown stream type {}",
				rule.stream_type.to_i32()
			));
		}
	};
	let matcher = match &rule.r#match {
		Some(Match::Language(language)) => Matcher::Language(language.clone()),
		Some(Match::Codec(codec)) => Matcher::Codec(codec.clone()),
		Some(Match::StreamIndex(index)) => Matcher::Stream(*index),
		None => return Err("needs a language, codec or stream index to match".into()),
	};
	match rule.action.as_known() {
		Some(RuleAction::Drop) => Ok(Rule::drop(stream_type, matcher)),
		Some(RuleAction::Keep) => Ok(Rule::keep(stream_type, matcher)),
		Some(RuleAction::Unspecified) => Err("needs an action: drop or keep".into()),
		None => Err(format!("has an unknown action {}", rule.action.to_i32())),
	}
}

/// The output path: `output` itself when absolute, or next to `input` when
/// it is a bare file name.
pub(crate) fn output_path(input: &Path, output: &str) -> Result<PathBuf, ConnectError> {
	let path = Path::new(output);
	if output.is_empty() {
		return Err(ConnectError::invalid_argument("output path is required"));
	}
	if path.is_absolute() {
		return Ok(path.to_path_buf());
	}
	let mut components = path.components();
	match (components.next(), components.next()) {
		(Some(Component::Normal(name)), None) => {
			Ok(input.parent().unwrap_or_else(|| Path::new("/")).join(name))
		}
		_ => Err(ConnectError::invalid_argument(format!(
			"output must be an absolute path or a bare file name: {output}"
		))),
	}
}

/// The output extensions the containers claim, for a message.
pub(crate) fn extensions() -> String {
	streams::CONTAINERS
		.iter()
		.flat_map(|c| c.extensions().iter().map(|e| format!(".{e}")))
		.collect::<Vec<_>>()
		.join(", ")
}

/// The file name of `path`, for the job's label and the preview.
pub(crate) fn file_name(path: &Path) -> String {
	path.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_bare_name_goes_next_to_the_input() {
		let input = Path::new("/media/in/movie.mkv");
		assert_eq!(
			output_path(input, "movie.mp4").unwrap(),
			PathBuf::from("/media/in/movie.mp4")
		);
		assert_eq!(
			output_path(input, "/media/out/movie.mp4").unwrap(),
			PathBuf::from("/media/out/movie.mp4")
		);
		assert!(output_path(input, "").is_err());
		assert!(output_path(input, "out/movie.mp4").is_err());
		assert!(output_path(input, "../movie.mp4").is_err());
		assert!(output_path(input, ".").is_err());
	}

	#[test]
	fn the_extensions_are_listed_for_the_message() {
		assert_eq!(
			extensions(),
			".mkv, .mka, .mks, .mp4, .m4v, .m4a, .mov, .webm"
		);
	}
}
