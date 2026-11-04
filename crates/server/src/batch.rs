//! `BatchService`: one stream operation over every matching file of a
//! folder, one job per file.
//!
//! The logic is `muxmultitool_core::batch`: this module resolves the
//! folders (file ids or paths inside a media root), finds the files with
//! [`MediaRoots::find`], probes them, and maps the plan to the messages.
//! `PlanBatch` is the dry run; `SubmitBatch` runs the same plan and queues
//! one job per ready item on the shared queue, so each file has its own
//! progress and its own failure.
//!
//! Outputs stay inside the media roots: the output folder goes through
//! the same checks as any file id, and an absolute path is accepted only
//! inside a root (or in desktop mode, where native-dialog paths are
//! trusted, as for every other operation).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::batch::{
	self, Batch, Destination, Existing, Input, Item, Streams, Template, utc_date,
};
use muxmultitool_core::ffmpeg::Limits;
use muxmultitool_core::ffmpeg::command::ShellLine;
use muxmultitool_core::files::{self, Entry, FileId, Kind, Location, MediaRoots};
use muxmultitool_core::jobs::JobSpec;
use muxmultitool_core::report;
use muxmultitool_core::streams::Selection;
use muxmultitool_proto::connect::muxmultitool::batch::v1::BatchService;
use muxmultitool_proto::proto::muxmultitool::batch::v1::{
	self as proto, BatchOperation, PlanBatchRequest, PlanBatchResponse, SubmitBatchRequest,
	SubmitBatchResponse,
};
use muxmultitool_proto::proto::muxmultitool::streams::v1::{self as streams_proto, SelectionMode};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::warn;

use crate::AppState;
use crate::files::{connect_error, entry as entry_message};
use crate::jobs::{Jobs, PROBE_TIMEOUT, job_message, queue_error};
use crate::streams::{file_name, rules};

/// How many files a plan probes at once.
const PROBE_CONCURRENCY: usize = 4;

/// Implements the batch service. It reuses the jobs service's path checks,
/// so a plan and a submit go through the same code.
#[derive(Debug)]
pub(crate) struct Batches {
	jobs: Jobs,
}

impl Batches {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}
}

impl BatchService for Batches {
	async fn plan_batch(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, PlanBatchRequest>,
	) -> ServiceResult<PlanBatchResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation.into_option() else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let planned = self.plan(&operation).await?;
		let items: Vec<proto::BatchItem> = planned
			.rows()
			.map(|(entry, input, item)| self.item_message(entry, input, item))
			.collect();
		Ok(Response::new(PlanBatchResponse {
			ready: u32::try_from(items.iter().filter(|i| i.ready).count()).unwrap_or(u32::MAX),
			items,
			date: planned.date,
			..Default::default()
		}))
	}

	async fn submit_batch(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, SubmitBatchRequest>,
	) -> ServiceResult<SubmitBatchResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation.into_option() else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let planned = self.plan(&operation).await?;
		if planned.items.is_empty() {
			return Err(ConnectError::failed_precondition(format!(
				"no file in {} matches {}",
				planned.folder.display(),
				operation.patterns.join(", ")
			)));
		}
		let mut jobs = Vec::new();
		let mut skipped = Vec::new();
		for (entry, input, item) in planned.rows() {
			let (Some(ready), Some(command), Some(output)) =
				(item.ready(), item.command(), item.output.as_deref())
			else {
				skipped.push(self.item_message(entry, input, item));
				continue;
			};
			let spec = JobSpec {
				name: file_name(output),
				command,
				expected_duration: ready.expected_duration,
				preparation: Vec::new(),
				scratch_dir: None,
			};
			let job = self
				.jobs
				.state
				.queue
				.submit(spec)
				.map_err(|error| queue_error(&error))?;
			jobs.push(job_message(&job));
		}
		Ok(Response::new(SubmitBatchResponse {
			jobs,
			skipped,
			..Default::default()
		}))
	}
}

/// A batch resolved against its folder: the files found, their probes and
/// the plan, all in the same order.
struct Planned {
	folder: PathBuf,
	entries: Vec<Entry>,
	inputs: Vec<Input>,
	items: Vec<Item>,
	date: String,
}

impl Planned {
	fn rows(&self) -> impl Iterator<Item = (&Entry, &Input, &Item)> {
		self.entries
			.iter()
			.zip(&self.inputs)
			.zip(&self.items)
			.map(|((entry, input), item)| (entry, input, item))
	}
}

impl Batches {
	/// Validates the operation, finds and probes the files and plans the
	/// batch. The request's mistakes are `INVALID_ARGUMENT`; a file's own
	/// problem is a skipped item, never an error.
	async fn plan(&self, operation: &BatchOperation) -> Result<Planned, ConnectError> {
		let template = Template::parse(&operation.template)
			.map_err(|error| ConnectError::invalid_argument(error.to_string()))?;
		let streams = streams(operation)?;
		if operation.patterns.is_empty() {
			return Err(connect_error(files::Error::NoPatterns));
		}
		let (folder_id, folder) = self.folder(&operation.folder, "folder").await?;
		let destination = if operation.output_folder.is_empty() {
			Destination::NextToInput
		} else {
			Destination::Folder(
				self.folder(&operation.output_folder, "output_folder")
					.await?
					.1,
			)
		};
		let batch = Batch {
			destination,
			template,
			existing: if operation.overwrite {
				Existing::Overwrite
			} else {
				Existing::Skip
			},
			streams,
		};

		let patterns = operation.patterns.clone();
		let recursive = operation.recursive;
		let state = Arc::clone(&self.jobs.state);
		let found: Vec<(Entry, PathBuf)> = blocking(move || {
			state
				.files
				.find(&folder_id, &patterns, recursive)?
				.into_iter()
				.map(|entry| {
					let path = state.files.path(&entry.id)?;
					Ok((entry, path))
				})
				.collect()
		})
		.await?;
		let (entries, paths): (Vec<Entry>, Vec<PathBuf>) = found.into_iter().unzip();
		let inputs = self.probe_all(paths).await?;
		let date = utc_date(SystemTime::now());
		let items = batch::plan(&batch, &inputs, &date);
		Ok(Planned {
			folder,
			entries,
			inputs,
			items,
			date,
		})
	}

	/// Probes every path, a few at a time, keeping the order. A probe that
	/// fails is an [`Input`] with the error, not a failure of the plan.
	async fn probe_all(&self, paths: Vec<PathBuf>) -> Result<Vec<Input>, ConnectError> {
		let slots = Arc::new(Semaphore::new(PROBE_CONCURRENCY));
		let mut tasks = JoinSet::new();
		for (i, path) in paths.into_iter().enumerate() {
			let runner = self.jobs.state.runner.clone();
			let slots = Arc::clone(&slots);
			tasks.spawn(async move {
				let _slot = slots.acquire_owned().await;
				let probe = runner
					.probe(&path, &Limits::timeout(PROBE_TIMEOUT))
					.await
					.map_err(|error| error.to_string());
				(i, Input { path, probe })
			});
		}
		let mut inputs: Vec<Option<Input>> =
			std::iter::repeat_with(|| None).take(tasks.len()).collect();
		while let Some(joined) = tasks.join_next().await {
			let (i, input) = joined.map_err(|error| {
				warn!(%error, "probe task failed");
				ConnectError::internal("the probe task failed")
			})?;
			inputs[i] = Some(input);
		}
		Ok(inputs.into_iter().flatten().collect())
	}

	/// A folder given as a file id or as an absolute path: its id, for
	/// [`MediaRoots::find`], and its real path.
	async fn folder(&self, given: &str, what: &str) -> Result<(FileId, PathBuf), ConnectError> {
		if given.is_empty() {
			return Err(ConnectError::invalid_argument(format!(
				"{what} is required"
			)));
		}
		let id = FileId::new(given);
		let id = if id.decode().is_ok() {
			id
		} else {
			self.folder_id_for_path(given, what)?
		};
		let state = Arc::clone(&self.jobs.state);
		let (entry, path) = blocking(move || {
			let entry = state.files.get(&id)?;
			let path = state.files.path(&id)?;
			Ok((entry, path))
		})
		.await?;
		if entry.kind != Kind::Folder {
			return Err(ConnectError::invalid_argument(format!(
				"{what} is a file, not a folder: {}",
				entry.name
			)));
		}
		Ok((entry.id, path))
	}

	/// The id of an absolute folder path: a native-dialog path in desktop
	/// mode, otherwise a path inside one of the media roots.
	fn folder_id_for_path(&self, given: &str, what: &str) -> Result<FileId, ConnectError> {
		let path = Path::new(given);
		if !path.is_absolute() {
			return Err(ConnectError::invalid_argument(format!(
				"{what} must be a folder id or an absolute path: {given}"
			)));
		}
		if self.jobs.state.files.accepts_paths() {
			return Ok(Location::Absolute(path.to_path_buf()).id());
		}
		let canonical = path.canonicalize().map_err(|_| {
			ConnectError::not_found(format!("{what} not found: {}", path.display()))
		})?;
		self.jobs.inside_media_roots(&canonical, what)?;
		location_in_roots(&self.jobs.state.files, &canonical)
			.map(|location| location.id())
			.ok_or_else(|| {
				ConnectError::permission_denied(format!(
					"{what} is outside the media roots: {}",
					canonical.display()
				))
			})
	}

	/// One item of the plan as a message.
	fn item_message(&self, entry: &Entry, input: &Input, item: &Item) -> proto::BatchItem {
		let ready = item.ready();
		let streams = match (&input.probe, ready) {
			(Ok(probe), Some(ready)) => report::build(probe, None)
				.streams
				.into_iter()
				.zip(&ready.plan.decisions)
				.zip(&ready.whys)
				.map(|((stream, decision), why)| streams_proto::ResultStream {
					stream: Some(crate::report::stream(stream)).into(),
					kept: decision.kept,
					why: why.clone(),
					..Default::default()
				})
				.collect(),
			_ => Vec::new(),
		};
		let command = item.command().map(|command| {
			ShellLine(&self.jobs.state.runner.ffmpeg_invocation(&command)).to_string()
		});
		proto::BatchItem {
			input: Some(entry_message(entry.clone())).into(),
			output: item
				.output
				.as_ref()
				.map(|o| o.display().to_string())
				.unwrap_or_default(),
			output_name: item.output.as_deref().map(file_name).unwrap_or_default(),
			ready: ready.is_some(),
			skip: item.skip().map(ToString::to_string).unwrap_or_default(),
			overwrites: ready.is_some_and(|r| r.overwrites),
			container: ready.map_or_else(String::new, |r| r.plan.container.name().to_owned()),
			streams,
			issues: ready
				.into_iter()
				.flat_map(|r| &r.plan.issues)
				.map(|issue| streams_proto::StreamIssue {
					stream_index: issue.index,
					blocking: issue.blocking,
					message: issue.message.clone(),
					..Default::default()
				})
				.collect(),
			warnings: ready.map(|r| r.warnings.clone()).unwrap_or_default(),
			command: command.unwrap_or_default(),
			duration_ms: ready
				.and_then(|r| r.expected_duration)
				.and_then(|d| i64::try_from(d.as_millis()).ok())
				.unwrap_or_default(),
			..Default::default()
		}
	}
}

/// Which streams of every file are copied, from the operation's mode.
fn streams(operation: &BatchOperation) -> Result<Streams, ConnectError> {
	let indexes = operation.stream_indexes.iter().copied();
	Ok(match operation.mode.as_known() {
		Some(SelectionMode::Unspecified | SelectionMode::All) => {
			Streams::Selection(Selection::all())
		}
		Some(SelectionMode::Keep) => Streams::Selection(Selection::keep(indexes)),
		Some(SelectionMode::Drop) => Streams::Selection(Selection::drop(indexes)),
		Some(SelectionMode::VideoOnly) => Streams::Selection(Selection::video_only()),
		Some(SelectionMode::Rules) => {
			let rules = rules(operation.rules.as_option())?;
			rules
				.validate()
				.map_err(|error| ConnectError::invalid_argument(error.to_string()))?;
			Streams::Rules(rules)
		}
		None => {
			return Err(ConnectError::invalid_argument(format!(
				"unknown selection mode {}",
				operation.mode.to_i32()
			)));
		}
	})
}

/// The location of a canonical path under one of the roots, if any.
fn location_in_roots(files: &MediaRoots, canonical: &Path) -> Option<Location> {
	files
		.roots()
		.iter()
		.enumerate()
		.filter_map(|(root, path)| path.canonicalize().ok().map(|c| (root, c)))
		.find_map(|(root, canonical_root)| {
			canonical
				.strip_prefix(&canonical_root)
				.ok()
				.map(|relative| Location::InRoot {
					root,
					relative: relative.to_path_buf(),
				})
		})
}

/// Runs file-system work on the blocking pool.
async fn blocking<T: Send + 'static>(
	work: impl FnOnce() -> Result<T, files::Error> + Send + 'static,
) -> Result<T, ConnectError> {
	tokio::task::spawn_blocking(work)
		.await
		.map_err(|error| {
			warn!(%error, "files task failed");
			ConnectError::internal("the files task failed")
		})?
		.map_err(connect_error)
}
