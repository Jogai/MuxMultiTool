//! The queue itself: submission, scheduling, cancellation, watching and
//! shutdown.
//!
//! All state sits behind one mutex that is never held across an `await`.
//! Events are pushed to the job's buffer and broadcast under that lock, so a
//! watch that subscribes under the same lock sees exactly the events after
//! its snapshot: no gap and no duplicate.
//!
//! Each running job is a tokio task that drives [`Runner::run_with_log`]
//! and forwards progress and log lines as events. Cancelling a job cancels
//! its token; the runner then kills ffmpeg and the task reports the end.
//! [`Queue::shutdown`] does that for every job and waits for the tasks, so
//! no ffmpeg process outlives the server.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::events::{EVENT_BUFFER, Event, EventBuffer, EventKind, Snapshot, WatchItem, can_resume};
use super::{JOB_LOG_LINES, Job, JobError, JobId, JobProgress, JobSpec, JobState, explain};
use crate::ffmpeg::command::ShellLine;
use crate::ffmpeg::{FfmpegCommand, Limits, Progress, RunError, Runner};

/// How many events a slow watch may fall behind before it is sent a fresh
/// snapshot instead of the events it missed.
const BROADCAST_CAPACITY: usize = 1024;

/// Settings of a [`Queue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueConfig {
	/// How many jobs run at the same time. `0` counts as `1`.
	pub concurrency: usize,
}

impl Default for QueueConfig {
	/// One job at a time: ffmpeg already uses every core, and two jobs on
	/// one disk are slower than one after the other.
	fn default() -> Self {
		Self { concurrency: 1 }
	}
}

/// What can go wrong when asking the queue for something.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
	/// No job has this id.
	#[error("job {0} not found")]
	NotFound(JobId),
	/// The job is already in a terminal state.
	#[error("job {0} has already {1}")]
	Finished(JobId, JobState),
	/// The server is shutting down and takes no new jobs.
	#[error("the server is shutting down")]
	ShuttingDown,
}

/// The job queue. Cheap to clone; every clone is the same queue.
///
/// Jobs are started with [`tokio::spawn`], so the queue must be used from
/// inside a tokio runtime.
#[derive(Debug, Clone)]
pub struct Queue {
	shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
	runner: Runner,
	concurrency: usize,
	inner: Mutex<Inner>,
	/// Cancelled at the end of [`Queue::shutdown`]; ends every watch.
	closed: CancellationToken,
	/// The tasks that run jobs, so shutdown can wait for them.
	tasks: TaskTracker,
	/// Makes job ids differ between server runs: a client that reconnects
	/// after a restart must not mistake a new job for an old one.
	id_prefix: String,
}

#[derive(Debug)]
struct Inner {
	/// Every job ever submitted, oldest first.
	jobs: Vec<Record>,
	/// The id the next event gets.
	next_event: u64,
	/// The number the next job gets.
	next_job: u64,
	/// How many jobs are in [`JobState::Running`].
	running: usize,
	/// Live events, for watches. Sent under the lock, see the module doc.
	events: broadcast::Sender<Arc<Event>>,
	/// `false` once shutdown started.
	accepting: bool,
}

/// One job and what the queue needs to run and watch it.
#[derive(Debug)]
struct Record {
	job: Job,
	/// The commands in order: the preparation, then the command.
	steps: Vec<FfmpegCommand>,
	scratch_dir: Option<PathBuf>,
	expected_duration: Option<Duration>,
	cancel: CancellationToken,
	buffer: EventBuffer,
}

impl Queue {
	/// A queue that runs jobs with `runner`.
	#[must_use]
	pub fn new(runner: Runner, config: QueueConfig) -> Self {
		let (events, _) = broadcast::channel(BROADCAST_CAPACITY);
		let since_epoch = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap_or_default()
			.as_millis();
		Self {
			shared: Arc::new(Shared {
				runner,
				concurrency: config.concurrency.max(1),
				inner: Mutex::new(Inner {
					jobs: Vec::new(),
					next_event: 1,
					next_job: 1,
					running: 0,
					events,
					accepting: true,
				}),
				closed: CancellationToken::new(),
				tasks: TaskTracker::new(),
				id_prefix: format!("{since_epoch:x}"),
			}),
		}
	}

	/// The runner that starts the jobs' ffmpeg processes.
	#[must_use]
	pub fn runner(&self) -> &Runner {
		&self.shared.runner
	}

	/// Queues `spec` and returns the new job, already with its exact
	/// command line. It starts at once when a slot is free.
	///
	/// # Errors
	/// [`QueueError::ShuttingDown`] once [`Queue::shutdown`] has begun.
	pub fn submit(&self, spec: JobSpec) -> Result<Job, QueueError> {
		let mut steps = spec.preparation;
		steps.push(spec.command);
		let mut command_line = String::new();
		let mut argv: Vec<String> = Vec::new();
		for (i, step) in steps.iter().enumerate() {
			let step = self.shared.runner.ffmpeg_invocation(step);
			if i > 0 {
				command_line.push_str(" && ");
				argv.push("&&".to_owned());
			}
			command_line.push_str(&ShellLine(&step).to_string());
			argv.extend(step.iter().map(|arg| arg.to_string_lossy().into_owned()));
		}
		let mut inner = self.shared.lock();
		if !inner.accepting {
			return Err(QueueError::ShuttingDown);
		}
		let id = JobId::from(format!("{}-{}", self.shared.id_prefix, inner.next_job));
		inner.next_job += 1;
		let job = Job {
			id: id.clone(),
			name: spec.name,
			state: JobState::Queued,
			argv,
			command_line,
			progress: None,
			log: Vec::new(),
			error: None,
			created: SystemTime::now(),
			started: None,
			finished: None,
		};
		inner.jobs.push(Record {
			job: job.clone(),
			steps,
			scratch_dir: spec.scratch_dir,
			expected_duration: spec.expected_duration,
			cancel: CancellationToken::new(),
			buffer: EventBuffer::new(EVENT_BUFFER),
		});
		tracing::info!(job = %id, command = %job.command_line, "job queued");
		inner.emit(
			&id,
			EventKind::State {
				state: JobState::Queued,
				error: None,
				at: job.created,
			},
		);
		self.shared.schedule(&mut inner);
		Ok(inner.find(&id).map_or(job, |record| record.job.clone()))
	}

	/// Every job, oldest first.
	#[must_use]
	pub fn list(&self) -> Vec<Job> {
		self.shared
			.lock()
			.jobs
			.iter()
			.map(|r| r.job.clone())
			.collect()
	}

	/// One job, or `None` when the id is unknown.
	#[must_use]
	pub fn get(&self, id: &JobId) -> Option<Job> {
		self.shared.lock().find(id).map(|r| r.job.clone())
	}

	/// Cancels a job. A queued job is cancelled at once; a running one is
	/// told to stop and becomes [`JobState::Cancelled`] once ffmpeg is gone,
	/// which the returned job (still running) does not show yet.
	///
	/// # Errors
	/// [`QueueError::NotFound`] for an unknown id, [`QueueError::Finished`]
	/// for a job that already ended.
	pub fn cancel(&self, id: &JobId) -> Result<Job, QueueError> {
		let mut inner = self.shared.lock();
		let record = inner
			.find_mut(id)
			.ok_or_else(|| QueueError::NotFound(id.clone()))?;
		match record.job.state {
			JobState::Queued => {
				tracing::info!(job = %id, "job cancelled before it started");
				inner.end(id, JobState::Cancelled, None);
			}
			JobState::Running => {
				tracing::info!(job = %id, "stopping job");
				record.cancel.cancel();
			}
			state => return Err(QueueError::Finished(id.clone(), state)),
		}
		inner
			.find(id)
			.map(|r| r.job.clone())
			.ok_or_else(|| QueueError::NotFound(id.clone()))
	}

	/// Follows one job: a snapshot (or, when `after` can be resumed, the
	/// missed events) and then every change until the job ends.
	///
	/// # Errors
	/// [`QueueError::NotFound`] for an unknown id.
	pub fn watch_job(&self, id: &JobId, after: u64) -> Result<Watch, QueueError> {
		let inner = self.shared.lock();
		inner
			.find(id)
			.ok_or_else(|| QueueError::NotFound(id.clone()))?;
		Ok(Watch::subscribe(
			Arc::clone(&self.shared),
			&inner,
			Scope::Job(id.clone()),
			after,
		))
	}

	/// Follows every job: a snapshot of the queue (or the missed events) and
	/// then every change, until the queue shuts down.
	#[must_use]
	pub fn watch_queue(&self, after: u64) -> Watch {
		let inner = self.shared.lock();
		Watch::subscribe(Arc::clone(&self.shared), &inner, Scope::Queue, after)
	}

	/// Stops taking jobs, cancels the queued and running ones, waits until
	/// every ffmpeg process is gone and then ends every watch.
	pub async fn shutdown(&self) {
		{
			let mut inner = self.shared.lock();
			inner.accepting = false;
			let ids: Vec<JobId> = inner.jobs.iter().map(|r| r.job.id.clone()).collect();
			for id in &ids {
				let Some(record) = inner.find_mut(id) else {
					continue;
				};
				match record.job.state {
					JobState::Queued => inner.end(id, JobState::Cancelled, None),
					JobState::Running => record.cancel.cancel(),
					_ => {}
				}
			}
		}
		self.shared.tasks.close();
		self.shared.tasks.wait().await;
		self.shared.closed.cancel();
		tracing::info!("job queue stopped");
	}

	/// Resolves once [`Queue::shutdown`] has finished.
	pub async fn closed(&self) {
		self.shared.closed.cancelled().await;
	}
}

impl Shared {
	fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
		self.inner.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// Starts queued jobs while there are free slots. Called with the lock
	/// held whenever a job is submitted or ends.
	fn schedule(self: &Arc<Self>, inner: &mut Inner) {
		while inner.accepting && inner.running < self.concurrency {
			let Some(record) = inner
				.jobs
				.iter_mut()
				.find(|r| r.job.state == JobState::Queued)
			else {
				return;
			};
			let now = SystemTime::now();
			record.job.state = JobState::Running;
			record.job.started = Some(now);
			let id = record.job.id.clone();
			let steps = record.steps.clone();
			let scratch_dir = record.scratch_dir.clone();
			let cancel = record.cancel.clone();
			inner.running += 1;
			inner.emit(
				&id,
				EventKind::State {
					state: JobState::Running,
					error: None,
					at: now,
				},
			);
			tracing::info!(job = %id, "job started");
			self.tasks
				.spawn(Arc::clone(self).run_job(id, steps, scratch_dir, cancel));
		}
	}

	/// Runs one job to its end and reports it: every step in order until
	/// one fails, then the scratch folder is removed.
	async fn run_job(
		self: Arc<Self>,
		id: JobId,
		steps: Vec<FfmpegCommand>,
		scratch_dir: Option<PathBuf>,
		cancel: CancellationToken,
	) {
		let limits = Limits::cancellable(cancel);
		let count = steps.len();
		let mut result = Ok(());
		for (step, command) in steps.into_iter().enumerate() {
			if count > 1 {
				tracing::info!(job = %id, step = step + 1, count, "job step started");
			}
			result = self.run_step(&id, &command, &limits, (step, count)).await;
			if result.is_err() {
				break;
			}
		}
		if let Some(dir) = scratch_dir
			&& let Err(error) = tokio::fs::remove_dir_all(&dir).await
			&& error.kind() != std::io::ErrorKind::NotFound
		{
			tracing::warn!(job = %id, dir = %dir.display(), %error, "scratch folder not removed");
		}
		let (state, error) = match result {
			Ok(()) => (JobState::Succeeded, None),
			Err(RunError::Cancelled { .. }) => (JobState::Cancelled, None),
			Err(RunError::Failed {
				status, log_tail, ..
			}) => (
				JobState::Failed,
				Some(JobError {
					message: explain(status, &log_tail),
					ffmpeg_output: log_tail,
				}),
			),
			Err(error) => (
				JobState::Failed,
				Some(JobError {
					message: error.to_string(),
					ffmpeg_output: String::new(),
				}),
			),
		};
		if let Some(error) = &error {
			tracing::warn!(job = %id, %state, message = %error.message, "job ended");
		} else {
			tracing::info!(job = %id, %state, "job ended");
		}
		let mut inner = self.lock();
		inner.running = inner.running.saturating_sub(1);
		inner.end(&id, state, error);
		self.schedule(&mut inner);
	}

	/// Runs one command of a job, forwarding its progress and log lines.
	/// `step` is the command's position and the count, for the
	/// percentage.
	async fn run_step(
		&self,
		id: &JobId,
		command: &FfmpegCommand,
		limits: &Limits,
		step: (usize, usize),
	) -> Result<(), RunError> {
		let (progress_tx, mut progress_rx) = mpsc::channel::<Progress>(32);
		let (log_tx, mut log_rx) = mpsc::channel::<String>(256);
		let run = self
			.runner
			.run_with_log(command, limits, Some(progress_tx), Some(log_tx));
		let forward_progress = async {
			while let Some(report) = progress_rx.recv().await {
				self.progress(id, report, step);
			}
		};
		let forward_log = async {
			while let Some(line) = log_rx.recv().await {
				self.log(id, line);
			}
		};
		let (result, (), ()) = tokio::join!(run, forward_progress, forward_log);
		result.map(drop)
	}

	fn progress(&self, id: &JobId, report: Progress, (step, count): (usize, usize)) {
		let mut inner = self.lock();
		let Some(record) = inner.find_mut(id) else {
			return;
		};
		let mut progress = JobProgress::new(report, record.expected_duration);
		if count > 1 {
			// Each step is an equal share of the whole.
			#[expect(clippy::cast_precision_loss, reason = "a handful of steps")]
			let (step, count) = (step as f64, count as f64);
			progress.percent = progress
				.percent
				.map(|percent| (step * 100.0 + percent) / count);
		}
		record.job.progress = Some(progress.clone());
		inner.emit(id, EventKind::Progress(progress));
	}

	fn log(&self, id: &JobId, line: String) {
		let mut inner = self.lock();
		let Some(record) = inner.find_mut(id) else {
			return;
		};
		if record.job.log.len() == JOB_LOG_LINES {
			record.job.log.remove(0);
		}
		record.job.log.push(line.clone());
		inner.emit(id, EventKind::Log(line));
	}

	/// The current state of what `scope` covers, with the id of the newest
	/// event. Called with the lock held.
	fn snapshot(inner: &Inner, scope: &Scope) -> Snapshot {
		let jobs = match scope {
			Scope::Job(id) => inner.find(id).map(|r| r.job.clone()).into_iter().collect(),
			Scope::Queue => inner.jobs.iter().map(|r| r.job.clone()).collect(),
		};
		Snapshot {
			event_id: inner.latest_event(),
			jobs,
		}
	}
}

impl Inner {
	fn find(&self, id: &JobId) -> Option<&Record> {
		self.jobs.iter().find(|r| &r.job.id == id)
	}

	fn find_mut(&mut self, id: &JobId) -> Option<&mut Record> {
		self.jobs.iter_mut().find(|r| &r.job.id == id)
	}

	/// The id of the newest event; 0 before the first.
	fn latest_event(&self) -> u64 {
		self.next_event - 1
	}

	/// Records `kind` for job `id` and broadcasts it.
	fn emit(&mut self, id: &JobId, kind: EventKind) {
		let event = Arc::new(Event {
			id: self.next_event,
			job: id.clone(),
			kind,
		});
		self.next_event += 1;
		if let Some(record) = self.find_mut(id) {
			record.buffer.push(Arc::clone(&event));
		}
		// An error only means nobody is watching right now.
		let _ = self.events.send(event);
	}

	/// Moves job `id` to the terminal `state` and reports it.
	fn end(&mut self, id: &JobId, state: JobState, error: Option<JobError>) {
		let now = SystemTime::now();
		let Some(record) = self.find_mut(id) else {
			return;
		};
		if record.job.state.is_terminal() {
			return;
		}
		record.job.state = state;
		record.job.finished = Some(now);
		record.job.error.clone_from(&error);
		self.emit(
			id,
			EventKind::State {
				state,
				error,
				at: now,
			},
		);
	}
}

/// What a [`Watch`] follows.
#[derive(Debug, Clone)]
enum Scope {
	Job(JobId),
	Queue,
}

impl Scope {
	fn covers(&self, event: &Event) -> bool {
		match self {
			Self::Job(id) => &event.job == id,
			Self::Queue => true,
		}
	}
}

/// A live view of one job or of the queue, as a sequence of [`WatchItem`]s.
///
/// The first items are either a snapshot or the events the client missed
/// (see [`events`](super::events)). After that, every matching event
/// arrives as it happens. A job watch ends after the event that ends the
/// job; both kinds end when the queue shuts down.
#[derive(Debug)]
pub struct Watch {
	shared: Arc<Shared>,
	scope: Scope,
	/// The snapshot or replayed events still to deliver.
	pending: VecDeque<WatchItem>,
	live: broadcast::Receiver<Arc<Event>>,
	/// The highest id delivered so far; older live events are skipped,
	/// which keeps a resync snapshot free of duplicates.
	last_id: u64,
	/// `true` once the job watched had ended before the watch began: the
	/// watch ends when `pending` is drained, even if the client already saw
	/// the terminal event and nothing is pending.
	ends_after_pending: bool,
	done: bool,
}

impl Watch {
	/// Subscribes under the queue lock, so nothing between the snapshot
	/// (or replay) and the live receiver is lost or repeated.
	fn subscribe(shared: Arc<Shared>, inner: &Inner, scope: Scope, after: u64) -> Self {
		let live = inner.events.subscribe();
		let latest = inner.latest_event();
		let records: Vec<&Record> = match &scope {
			Scope::Job(id) => inner.find(id).into_iter().collect(),
			Scope::Queue => inner.jobs.iter().collect(),
		};
		let evicted = records
			.iter()
			.map(|r| r.buffer.newest_evicted())
			.max()
			.unwrap_or(0);
		let ends_after_pending =
			matches!(scope, Scope::Job(_)) && records.iter().all(|r| r.job.state.is_terminal());

		let (pending, last_id) = if can_resume(after, evicted, latest) {
			let mut missed: Vec<&Arc<Event>> =
				records.iter().flat_map(|r| r.buffer.after(after)).collect();
			missed.sort_by_key(|e| e.id);
			let items = missed
				.into_iter()
				.map(|e| WatchItem::Event(Arc::clone(e)))
				.collect();
			(items, after)
		} else {
			let snapshot = Shared::snapshot(inner, &scope);
			(VecDeque::from([WatchItem::Snapshot(snapshot)]), latest)
		};
		Self {
			shared,
			scope,
			pending,
			live,
			last_id,
			ends_after_pending,
			done: false,
		}
	}

	/// The next message, or `None` when the watch has ended.
	pub async fn next(&mut self) -> Option<WatchItem> {
		loop {
			if self.done {
				return None;
			}
			if let Some(item) = self.pending.pop_front() {
				return Some(self.deliver(item));
			}
			if self.ends_after_pending {
				self.done = true;
				return None;
			}
			tokio::select! {
				biased;
				received = self.live.recv() => match received {
					Ok(event) => {
						if self.scope.covers(&event) && event.id > self.last_id {
							return Some(self.deliver(WatchItem::Event(event)));
						}
					}
					Err(RecvError::Lagged(skipped)) => {
						tracing::debug!(skipped, "watch fell behind, sending a snapshot");
						let inner = self.shared.lock();
						let snapshot = Shared::snapshot(&inner, &self.scope);
						drop(inner);
						self.pending.push_back(WatchItem::Snapshot(snapshot));
					}
					Err(RecvError::Closed) => return None,
				},
				() = self.shared.closed.cancelled() => return None,
			}
		}
	}

	/// Notes what `item` means for the rest of the watch and hands it out.
	fn deliver(&mut self, item: WatchItem) -> WatchItem {
		self.last_id = self.last_id.max(item.event_id());
		if let Scope::Job(_) = self.scope {
			let ended = match &item {
				WatchItem::Snapshot(snapshot) => {
					snapshot.jobs.iter().all(|j| j.state.is_terminal())
				}
				WatchItem::Event(event) => event.is_terminal(),
			};
			if ended {
				self.done = true;
			}
		}
		item
	}
}
