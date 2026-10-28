//! `MergeService` and the merge operation of `JobsService`.
//!
//! A merge runs as a job (`SubmitJob` with a `MergeOperation`); this module
//! turns the operation into a [`JobSpec`]. The logic is
//! `muxmultitool_core::merge`: the server only resolves paths, probes the
//! inputs, maps the core types to the messages, and writes the two files
//! the command reads besides the parts: the concat list and, when asked,
//! the chapters. They go under the system temporary folder, named after a
//! hash of the output path, so `CheckMerge` shows the very command the job
//! will run before anything is written.
//!
//! `CheckMerge` runs the same check as a submit without queueing anything,
//! so the frontend can show the compatibility table and the problems while
//! the user orders the files and picks which streams to leave out.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::ffmpeg::command::ShellLine;
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits};
use muxmultitool_core::jobs::JobSpec;
use muxmultitool_core::merge::{self, Check, Mismatch, Parameter, Part, Row, command};
use muxmultitool_proto::connect::muxmultitool::merge::v1::MergeService;
use muxmultitool_proto::proto::muxmultitool::files::v1::StreamType;
use muxmultitool_proto::proto::muxmultitool::merge::v1::{
	self as proto, CheckMergeRequest, CheckMergeResponse, MergeOperation,
};

use crate::AppState;
use crate::jobs::{Jobs, PROBE_TIMEOUT};

/// Implements the merge service. It reuses the jobs service's path checks
/// and spec builder, so a check and a submit go through the same code.
#[derive(Debug)]
pub(crate) struct Merge {
	jobs: Jobs,
}

impl Merge {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}
}

impl MergeService for Merge {
	async fn check_merge(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, CheckMergeRequest>,
	) -> ServiceResult<CheckMergeResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation.into_option() else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let checked = self.jobs.merge_check(&operation).await?;
		// The output is only shown in the command here; it need not exist.
		let output = merge_output(&checked.parts[0].path, &operation.output);
		let command = checked.command(&Sidecars::for_output(&output), &output)?;
		let argv = self.jobs.state.runner.ffmpeg_invocation(&command);
		Ok(Response::new(CheckMergeResponse {
			parts: checked.parts.iter().map(part_message).collect(),
			streams: checked.check.rows.iter().map(row_message).collect(),
			problems: checked.check.blocking().map(mismatch_message).collect(),
			can_merge: checked.check.can_merge(),
			total_duration_ms: millis(checked.check.total_duration),
			command: ShellLine(&argv).to_string(),
			..Default::default()
		}))
	}
}

impl Jobs {
	/// Validates a merge request and turns it into a job spec. The files
	/// must pass the compatibility check; otherwise the submit fails with
	/// `FAILED_PRECONDITION` and the problems. The concat list and the
	/// chapters are written here, so the job only needs ffmpeg.
	pub(crate) async fn merge_spec(
		&self,
		name: &str,
		operation: &MergeOperation,
	) -> Result<JobSpec, ConnectError> {
		let checked = self.merge_check(operation).await?;
		let output = self.checked_output(
			&merge_output(&checked.parts[0].path, &operation.output).to_string_lossy(),
		)?;
		if !checked.check.can_merge() {
			return Err(ConnectError::failed_precondition(format!(
				"the files cannot be merged as they are: {}",
				checked.check.reason()
			)));
		}
		let sidecars = Sidecars::for_output(&output);
		let command = checked.command(&sidecars, &output)?;
		sidecars
			.write(&checked.parts, checked.chapters)
			.await
			.map_err(|error| {
				ConnectError::internal(format!(
					"cannot write the merge list under {}: {error}",
					sidecars.list.display()
				))
			})?;
		Ok(JobSpec {
			name: job_name(name, &output),
			command,
			expected_duration: checked.check.total_duration,
			preparation: Vec::new(),
			scratch_dir: None,
		})
	}

	/// Resolves and probes the inputs and runs the compatibility check.
	async fn merge_check(&self, operation: &MergeOperation) -> Result<Checked, ConnectError> {
		if operation.inputs.len() < merge::MIN_PARTS {
			return Err(ConnectError::invalid_argument(
				merge::CheckError::TooFewParts.to_string(),
			));
		}
		let mut parts = Vec::with_capacity(operation.inputs.len());
		for input in &operation.inputs {
			let path = self.cut_input(input)?;
			let probe = self
				.state
				.runner
				.probe(&path, &Limits::timeout(PROBE_TIMEOUT))
				.await
				.map_err(|error| {
					ConnectError::invalid_argument(format!(
						"the input is not a media file: {error}"
					))
				})?;
			parts.push(Part::from_probe(&path, &probe));
		}
		let check = merge::check(&parts, &operation.drop_streams)
			.map_err(|error| ConnectError::invalid_argument(error.to_string()))?;
		Ok(Checked {
			parts,
			check,
			drop: operation.drop_streams.clone(),
			chapters: operation.chapters,
		})
	}
}

/// A request after the check: what the command is built from.
struct Checked {
	parts: Vec<Part>,
	check: Check,
	drop: Vec<u32>,
	chapters: bool,
}

impl Checked {
	/// The job's command, reading `sidecars` and writing `output`.
	fn command(&self, sidecars: &Sidecars, output: &Path) -> Result<FfmpegCommand, ConnectError> {
		if self.chapters && self.check.total_duration.is_none() {
			return Err(ConnectError::invalid_argument(
				"chapters need the duration of every file, and one is unknown",
			));
		}
		Ok(command::merge(
			&sidecars.list,
			self.chapters.then_some(sidecars.chapters.as_path()),
			&self.drop,
			output,
		))
	}
}

/// The files a merge command reads besides the parts, under the system
/// temporary folder. Their names come from the output path, so the same
/// request always gets the same command.
struct Sidecars {
	list: PathBuf,
	chapters: PathBuf,
}

impl Sidecars {
	fn for_output(output: &Path) -> Self {
		let mut hasher = DefaultHasher::new();
		output.hash(&mut hasher);
		let stem = format!("{:016x}", hasher.finish());
		let dir = std::env::temp_dir().join("muxmultitool").join("merge");
		Self {
			list: dir.join(format!("{stem}.txt")),
			chapters: dir.join(format!("{stem}.ffmeta")),
		}
	}

	/// Writes the concat list of `parts`, and their chapters when `chapters`.
	async fn write(&self, parts: &[Part], chapters: bool) -> std::io::Result<()> {
		if let Some(dir) = self.list.parent() {
			tokio::fs::create_dir_all(dir).await?;
		}
		tokio::fs::write(&self.list, command::concat_list(parts)).await?;
		if chapters {
			let text = command::chapters(parts).unwrap_or_default();
			tokio::fs::write(&self.chapters, text).await?;
		}
		Ok(())
	}
}

/// The output as the operation names it: absolute, a bare file name next to
/// the first part, or the default next to the first part when empty. A path
/// the check rejects later comes back unchanged.
fn merge_output(first: &Path, output: &str) -> PathBuf {
	let path = Path::new(output);
	if output.is_empty() {
		return default_output(first);
	}
	let mut components = path.components();
	match (components.next(), components.next()) {
		(Some(Component::Normal(_)), None) => first.with_file_name(path),
		_ => path.to_path_buf(),
	}
}

/// `<first part>-merged.<its extension>`, for a check without an output.
fn default_output(first: &Path) -> PathBuf {
	let stem = first
		.file_stem()
		.map_or_else(|| "merged".to_owned(), |s| s.to_string_lossy().into_owned());
	let name = match first.extension() {
		Some(ext) => format!("{stem}-merged.{}", ext.to_string_lossy()),
		None => format!("{stem}-merged"),
	};
	first.with_file_name(name)
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

/// Milliseconds for the API; 0 for `None`.
fn millis(duration: Option<Duration>) -> i64 {
	duration
		.and_then(|d| i64::try_from(d.as_millis()).ok())
		.unwrap_or_default()
}

fn part_message(part: &Part) -> proto::MergePart {
	proto::MergePart {
		path: part.path.to_string_lossy().into_owned(),
		name: part.name.clone(),
		duration_ms: millis(part.duration),
		layout: part.layout(),
		..Default::default()
	}
}

fn row_message(row: &Row) -> proto::MergeStream {
	proto::MergeStream {
		index: row.index,
		r#type: match row.kind {
			CodecType::Video => StreamType::Video,
			CodecType::Audio => StreamType::Audio,
			CodecType::Subtitle => StreamType::Subtitle,
			CodecType::Data => StreamType::Data,
			CodecType::Attachment => StreamType::Attachment,
			CodecType::Unknown => StreamType::Unspecified,
		}
		.into(),
		cells: row
			.cells
			.iter()
			.map(|cell| cell.clone().unwrap_or_default())
			.collect(),
		identical: row.identical(),
		dropped: row.dropped,
		mismatches: row.mismatches.iter().map(mismatch_message).collect(),
		..Default::default()
	}
}

fn mismatch_message(mismatch: &Mismatch) -> proto::MergeMismatch {
	proto::MergeMismatch {
		stream_index: mismatch.stream,
		part: u32::try_from(mismatch.part).unwrap_or(u32::MAX),
		parameter: match mismatch.parameter {
			Parameter::Codec => proto::MergeParameter::Codec,
			Parameter::Resolution => proto::MergeParameter::Resolution,
			Parameter::FrameRate => proto::MergeParameter::FrameRate,
			Parameter::PixelFormat => proto::MergeParameter::PixelFormat,
			Parameter::SampleRate => proto::MergeParameter::SampleRate,
			Parameter::ChannelLayout => proto::MergeParameter::ChannelLayout,
			Parameter::StreamLayout => proto::MergeParameter::StreamLayout,
		}
		.into(),
		expected: mismatch.expected.clone(),
		actual: mismatch.actual.clone(),
		message: mismatch.to_string(),
		..Default::default()
	}
}
