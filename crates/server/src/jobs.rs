//! `JobsService`: submits, lists, cancels and watches jobs.
//!
//! The queue lives in [`AppState`]; this module validates requests, turns
//! operations into [`JobSpec`]s and maps the core's job types to the
//! protobuf messages. The two watch methods wrap a core [`Watch`] in a
//! Connect server stream: one message per snapshot or event, ending when
//! the watch ends. A client reads with the generated TypeScript client in
//! the browser, or with `curl` and the `application/connect+json` envelope.

use std::future::{Future, ready};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use connectrpc::{
	ConnectError, RequestContext, Response, ServiceRequest, ServiceResult, ServiceStream,
};
use muxmultitool_core::ffmpeg::{Limits, operations};
use muxmultitool_core::jobs::{
	Event, EventKind, Job, JobError, JobId, JobProgress, JobSpec, JobState, QueueError, Watch,
	WatchItem,
};
use muxmultitool_proto::connect::muxmultitool::jobs::v1::JobsService;
use muxmultitool_proto::proto::muxmultitool::jobs::v1::__buffa::oneof::job_event::Event as EventMessage;
use muxmultitool_proto::proto::muxmultitool::jobs::v1::__buffa::oneof::submit_job_request::Operation;
use muxmultitool_proto::proto::muxmultitool::jobs::v1::__buffa::oneof::{
	watch_job_response, watch_queue_response,
};
use muxmultitool_proto::proto::muxmultitool::jobs::v1::{
	self as proto, CancelJobRequest, CancelJobResponse, GetJobRequest, GetJobResponse,
	ListJobsRequest, ListJobsResponse, RemuxOperation, SubmitJobRequest, SubmitJobResponse,
	WatchJobRequest, WatchJobResponse, WatchQueueRequest, WatchQueueResponse,
};

use crate::AppState;

/// How long a probe of the input may take before a submit is rejected.
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Implements the jobs service on top of the queue in [`AppState`].
#[derive(Debug)]
pub(crate) struct Jobs {
	pub(crate) state: Arc<AppState>,
}

impl JobsService for Jobs {
	async fn submit_job(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, SubmitJobRequest>,
	) -> ServiceResult<SubmitJobResponse> {
		let request = request.to_owned_message();
		let Some(operation) = request.operation else {
			return Err(ConnectError::invalid_argument("an operation is required"));
		};
		let spec = match operation {
			Operation::Remux(remux) => self.remux_spec(&request.name, &remux).await?,
			Operation::VideoToGif(gif) => self.video_to_gif_spec(&request.name, &gif).await?,
			Operation::GifToVideo(gif) => self.gif_to_video_spec(&request.name, &gif).await?,
			Operation::SelectStreams(op) => self.select_streams_spec(&request.name, &op).await?,
			Operation::Cut(cut) => self.cut_spec(&request.name, &cut).await?,
			Operation::Merge(merge) => self.merge_spec(&request.name, &merge).await?,
			Operation::Crop(crop) => self.crop_spec(&request.name, &crop).await?,
		};
		let job = self
			.state
			.queue
			.submit(spec)
			.map_err(|error| queue_error(&error))?;
		Ok(Response::new(SubmitJobResponse {
			job: job_message(&job).into(),
			..Default::default()
		}))
	}

	// The methods below await nothing, so they return ready futures instead
	// of being `async fn` (clippy::unused_async_trait_impl).
	fn list_jobs(
		&self,
		_ctx: RequestContext,
		_request: ServiceRequest<'_, ListJobsRequest>,
	) -> impl Future<Output = ServiceResult<ListJobsResponse>> {
		ready(Ok(Response::new(ListJobsResponse {
			jobs: self.state.queue.list().iter().map(job_message).collect(),
			..Default::default()
		})))
	}

	fn get_job(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, GetJobRequest>,
	) -> impl Future<Output = ServiceResult<GetJobResponse>> {
		let id = JobId::from(request.to_owned_message().job_id);
		ready(
			self.state
				.queue
				.get(&id)
				.ok_or_else(|| queue_error(&QueueError::NotFound(id)))
				.map(|job| {
					Response::new(GetJobResponse {
						job: job_message(&job).into(),
						..Default::default()
					})
				}),
		)
	}

	fn cancel_job(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, CancelJobRequest>,
	) -> impl Future<Output = ServiceResult<CancelJobResponse>> {
		let id = JobId::from(request.to_owned_message().job_id);
		ready(
			self.state
				.queue
				.cancel(&id)
				.map_err(|error| queue_error(&error))
				.map(|job| {
					Response::new(CancelJobResponse {
						job: job_message(&job).into(),
						..Default::default()
					})
				}),
		)
	}

	fn watch_job(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, WatchJobRequest>,
	) -> impl Future<Output = ServiceResult<ServiceStream<WatchJobResponse>>> {
		let request = request.to_owned_message();
		let id = JobId::from(request.job_id);
		ready(
			self.state
				.queue
				.watch_job(&id, request.after_event_id)
				.map_err(|error| queue_error(&error))
				.map(|watch| {
					Response::stream(stream(watch, |item| WatchJobResponse {
						message: Some(match item {
							WatchItem::Snapshot(snapshot) => watch_job_response::Message::Snapshot(
								Box::new(proto::JobSnapshot {
									event_id: snapshot.event_id,
									job: snapshot.jobs.first().map(job_message).into(),
									..Default::default()
								}),
							),
							WatchItem::Event(event) => {
								watch_job_response::Message::Event(Box::new(event_message(&event)))
							}
						}),
						..Default::default()
					}))
				}),
		)
	}

	fn watch_queue(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, WatchQueueRequest>,
	) -> impl Future<Output = ServiceResult<ServiceStream<WatchQueueResponse>>> {
		let after = request.to_owned_message().after_event_id;
		let watch = self.state.queue.watch_queue(after);
		ready(Ok(Response::stream(stream(watch, |item| {
			WatchQueueResponse {
				message: Some(match item {
					WatchItem::Snapshot(snapshot) => {
						watch_queue_response::Message::Snapshot(Box::new(proto::QueueSnapshot {
							event_id: snapshot.event_id,
							jobs: snapshot.jobs.iter().map(job_message).collect(),
							..Default::default()
						}))
					}
					WatchItem::Event(event) => {
						watch_queue_response::Message::Event(Box::new(event_message(&event)))
					}
				}),
				..Default::default()
			}
		}))))
	}
}

impl Jobs {
	/// Validates a remux request and turns it into a job spec. The input is
	/// probed for its duration, which gives the job a percentage and ETA,
	/// and the name defaults to the output file name.
	async fn remux_spec(
		&self,
		name: &str,
		remux: &RemuxOperation,
	) -> Result<JobSpec, ConnectError> {
		let input = self.checked_input(&remux.input)?;
		let output = self.checked_output(&remux.output)?;
		let probe = self
			.state
			.runner
			.probe(&input, &Limits::timeout(PROBE_TIMEOUT))
			.await
			.map_err(|error| {
				ConnectError::invalid_argument(format!("the input is not a media file: {error}"))
			})?;
		let name = if name.is_empty() {
			output
				.file_name()
				.map(|n| n.to_string_lossy().into_owned())
				.unwrap_or_default()
		} else {
			name.to_owned()
		};
		Ok(JobSpec {
			name,
			command: operations::remux(&input, &output),
			expected_duration: probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
		})
	}

	/// An existing file inside a media root, as its canonical path.
	pub(crate) fn checked_input(&self, path: &str) -> Result<PathBuf, ConnectError> {
		let path = absolute(path, "input")?;
		let canonical = path.canonicalize().map_err(|_| {
			ConnectError::not_found(format!("input file not found: {}", path.display()))
		})?;
		if !canonical.is_file() {
			return Err(ConnectError::invalid_argument(format!(
				"input is not a file: {}",
				path.display()
			)));
		}
		self.inside_media_roots(&canonical, "input")?;
		Ok(canonical)
	}

	/// A new file in an existing folder inside a media root, as the folder's
	/// canonical path plus the file name.
	pub(crate) fn checked_output(&self, path: &str) -> Result<PathBuf, ConnectError> {
		let path = absolute(path, "output")?;
		let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) else {
			return Err(ConnectError::invalid_argument(format!(
				"output must be a file path: {}",
				path.display()
			)));
		};
		let folder = parent.canonicalize().map_err(|_| {
			ConnectError::invalid_argument(format!(
				"output folder does not exist: {}",
				parent.display()
			))
		})?;
		self.inside_media_roots(&folder, "output")?;
		let output = folder.join(file_name);
		if output.exists() {
			return Err(ConnectError::already_exists(format!(
				"output file already exists: {}",
				output.display()
			)));
		}
		Ok(output)
	}

	/// Jobs may only read and write inside the configured media roots.
	/// `canonical` must already be canonical, so symlinks cannot escape.
	fn inside_media_roots(&self, canonical: &Path, what: &str) -> Result<(), ConnectError> {
		if self.state.media_roots.is_empty() {
			return Err(ConnectError::failed_precondition(
				"no media roots are configured; start the server with --media-root",
			));
		}
		let inside = self
			.state
			.media_roots
			.iter()
			.filter_map(|root| root.canonicalize().ok())
			.any(|root| canonical.starts_with(root));
		if inside {
			Ok(())
		} else {
			Err(ConnectError::permission_denied(format!(
				"{what} is outside the media roots: {}",
				canonical.display()
			)))
		}
	}
}

/// A non-empty absolute path. Relative paths are refused rather than
/// resolved against the server's working directory.
fn absolute(path: &str, what: &str) -> Result<PathBuf, ConnectError> {
	let path = Path::new(path);
	if path.as_os_str().is_empty() {
		return Err(ConnectError::invalid_argument(format!(
			"{what} path is required"
		)));
	}
	if !path.is_absolute() {
		return Err(ConnectError::invalid_argument(format!(
			"{what} path must be absolute: {}",
			path.display()
		)));
	}
	Ok(path.to_path_buf())
}

/// Wraps a watch in a Connect server stream, mapping each item with `map`.
/// The stream ends when the watch does; dropping it (the client went away)
/// drops the watch.
fn stream<M>(watch: Watch, map: impl Fn(WatchItem) -> M + Send + 'static) -> ServiceStream<M>
where
	M: Send + 'static,
{
	Box::pin(futures_util::stream::unfold(
		(watch, map),
		|(mut watch, map)| async move {
			let item = watch.next().await?;
			Some((Ok(map(item)), (watch, map)))
		},
	))
}

fn queue_error(error: &QueueError) -> ConnectError {
	match error {
		QueueError::NotFound(_) => ConnectError::not_found(error.to_string()),
		QueueError::Finished(..) => ConnectError::failed_precondition(error.to_string()),
		QueueError::ShuttingDown => ConnectError::unavailable(error.to_string()),
	}
}

fn job_message(job: &Job) -> proto::Job {
	proto::Job {
		id: job.id.as_str().to_owned(),
		name: job.name.clone(),
		state: state_message(job.state).into(),
		command: job.command_line.clone(),
		argv: job.argv.clone(),
		progress: job.progress.as_ref().map(progress_message).into(),
		log: job.log.clone(),
		error: job.error.as_ref().map(error_message).into(),
		created_unix_ms: unix_ms(Some(job.created)),
		started_unix_ms: unix_ms(job.started),
		finished_unix_ms: unix_ms(job.finished),
		..Default::default()
	}
}

fn state_message(state: JobState) -> proto::JobState {
	match state {
		JobState::Queued => proto::JobState::Queued,
		JobState::Running => proto::JobState::Running,
		JobState::Succeeded => proto::JobState::Succeeded,
		JobState::Failed => proto::JobState::Failed,
		JobState::Cancelled => proto::JobState::Cancelled,
	}
}

fn progress_message(progress: &JobProgress) -> proto::Progress {
	let report = &progress.report;
	proto::Progress {
		out_time_ms: report
			.out_time
			.and_then(|t| i64::try_from(t.as_millis()).ok())
			.unwrap_or_default(),
		speed: report.speed.unwrap_or_default(),
		fps: report.fps.unwrap_or_default(),
		frame: report
			.frame
			.and_then(|f| i64::try_from(f).ok())
			.unwrap_or_default(),
		total_size: report
			.total_size
			.and_then(|s| i64::try_from(s).ok())
			.unwrap_or_default(),
		bitrate_kbps: report.bitrate_kbps.unwrap_or_default(),
		percent: progress.percent,
		eta_ms: progress.eta.and_then(|t| i64::try_from(t.as_millis()).ok()),
		..Default::default()
	}
}

fn error_message(error: &JobError) -> proto::ErrorDetail {
	proto::ErrorDetail {
		message: error.message.clone(),
		ffmpeg_output: error.ffmpeg_output.clone(),
		..Default::default()
	}
}

fn event_message(event: &Event) -> proto::JobEvent {
	let kind = match &event.kind {
		EventKind::State { state, error, at } => {
			EventMessage::State(Box::new(proto::StateChange {
				state: state_message(*state).into(),
				error: error.as_ref().map(error_message).into(),
				at_unix_ms: unix_ms(Some(*at)),
				..Default::default()
			}))
		}
		EventKind::Progress(progress) => {
			EventMessage::Progress(Box::new(progress_message(progress)))
		}
		EventKind::Log(line) => EventMessage::LogLine(line.clone()),
	};
	proto::JobEvent {
		id: event.id,
		job_id: event.job.as_str().to_owned(),
		event: Some(kind),
		..Default::default()
	}
}

/// Milliseconds since the Unix epoch; 0 for `None`.
fn unix_ms(time: Option<SystemTime>) -> i64 {
	time.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
		.and_then(|d| i64::try_from(d.as_millis()).ok())
		.unwrap_or_default()
}
