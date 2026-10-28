//! On-demand H.264/AAC HLS previews of source files.
//!
//! Browsers and webviews play very little of what a media folder holds
//! (MKV, HEVC, DTS, ...), so the player never gets the source. It gets an
//! HLS stream that is H.264 + AAC in MPEG-TS segments: copied from the
//! source where it already is that, transcoded with x264 otherwise. Exports
//! never use the preview; they always read the original file.
//!
//! A [`PreviewManager`] holds one session per source file:
//!
//! 1. [`PreviewManager::start`] probes the file, decides what to copy and
//!    what to encode ([`plan`]) and fixes the [`Playlist`]: every segment of
//!    the whole file, so the player can seek anywhere from the start. For a
//!    stream copy this needs the source's keyframes, which costs one read of
//!    the file.
//! 2. [`PreviewManager::segment`] hands out segment files. A segment that
//!    does not exist yet is produced by a *run*: one ffmpeg process that
//!    writes consecutive segments from some segment on ([`command`]). A
//!    request the current run will reach soon waits for it; a request
//!    elsewhere (a seek) stops the run and starts one at that segment.
//!    Segments stay on disk, so going back is free.
//! 3. A run stops when no segment has been requested for
//!    [`PreviewConfig::run_idle`], and a session is removed with its
//!    segments after [`PreviewConfig::session_idle`] without requests, on
//!    [`PreviewManager::stop`], or when the manager is dropped.
//!
//! The playlist and the segments are served by the HTTP media routes, which
//! are outside this crate; this module only knows files and ids.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::ffmpeg::{Limits, RunError, Runner};
use crate::fs::ScratchRoot;

pub mod command;
pub mod plan;
pub mod playlist;

pub use plan::{Mode, Plan, Streams};
pub use playlist::{Playlist, Segment};

/// Keyframes closer than this after a candidate boundary disqualify it; see
/// [`Playlist::from_keyframes`] and [`command::SEEK_MARGIN_SECS`].
const KEYFRAME_GUARD_SECS: f64 = command::SEEK_MARGIN_SECS + 0.02;

/// How often a run checks its segment list and the idle clock.
const RUN_POLL: Duration = Duration::from_millis(100);

/// How long a stop waits for a run's ffmpeg to go away.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Tunables of the preview sessions. [`Default`] is what the server uses.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewConfig {
	/// Length of a transcoded segment, and the minimum length of a copied
	/// one. Shorter segments mean a faster start after a seek, because the
	/// player needs a whole segment before it plays.
	pub segment_secs: f64,
	/// Longest copied segment, which is the longest keyframe interval the
	/// copy accepts. A source with sparser keyframes is transcoded instead.
	pub max_copy_segment_secs: f64,
	/// A run stops when no segment was requested for this long.
	pub run_idle: Duration,
	/// A session and its segments are removed after this long without requests.
	pub session_idle: Duration,
	/// How many segments past the one a run is producing a request may
	/// wait for, before it is treated as a seek and gets its own run.
	pub lookahead: usize,
	/// How long a segment request waits for ffmpeg before it gives up.
	pub wait_timeout: Duration,
	/// Limit for the probe and the keyframe scan when a session starts.
	pub scan_timeout: Duration,
}

impl Default for PreviewConfig {
	fn default() -> Self {
		Self {
			segment_secs: 2.0,
			max_copy_segment_secs: 30.0,
			run_idle: Duration::from_secs(30),
			session_idle: Duration::from_secs(30 * 60),
			lookahead: 3,
			wait_timeout: Duration::from_secs(60),
			scan_timeout: Duration::from_secs(120),
		}
	}
}

/// What a client needs to know about a session.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewInfo {
	/// Opaque session id: the `{id}` of the media routes.
	pub id: String,
	/// How the preview is produced.
	pub mode: Mode,
	/// Length of the preview in seconds.
	pub duration: f64,
	/// Number of segments in the playlist.
	pub segment_count: usize,
}

/// Why a session could not be started.
#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
	/// The path cannot be resolved or read.
	#[error("cannot open {path}: {source}")]
	Open {
		/// The path as given.
		path: PathBuf,
		/// The OS error.
		#[source]
		source: io::Error,
	},
	/// The path is a folder or something else that is not a file.
	#[error("{0} is not a file")]
	NotAFile(PathBuf),
	/// ffprobe could not read the file.
	#[error("probing failed: {0}")]
	Probe(#[source] RunError),
	/// Nothing to play.
	#[error("the file has no video or audio stream")]
	NoStreams,
	/// Without a duration there is no playlist.
	#[error("the file has no known duration")]
	NoDuration,
	/// The session folder could not be created.
	#[error("cannot create the session folder: {0}")]
	Io(#[source] io::Error),
}

/// Why a segment could not be served.
#[derive(Debug, thiserror::Error)]
pub enum SegmentError {
	/// No session with that id; it may have expired.
	#[error("no preview session {0}")]
	UnknownSession(String),
	/// The playlist has no such segment.
	#[error("segment {0} is not in the playlist")]
	OutOfRange(usize),
	/// ffmpeg reached the end of the file before this segment: the probed
	/// duration was longer than the media.
	#[error("segment {0} is beyond the end of the media")]
	NotProduced(usize),
	/// ffmpeg did not deliver in time.
	#[error("segment {n} was not ready within {timeout:?}")]
	Timeout {
		/// The segment.
		n: usize,
		/// The wait limit.
		timeout: Duration,
	},
	/// ffmpeg failed; the text is the tail of its log.
	#[error("ffmpeg failed: {0}")]
	Failed(String),
}

/// The preview sessions of one server. Cheap to clone; clones share the sessions.
#[derive(Debug, Clone)]
pub struct PreviewManager {
	inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
	runner: Runner,
	config: PreviewConfig,
	/// Folder with one subfolder per session.
	base: ScratchRoot,
	sessions: Mutex<Sessions>,
}

#[derive(Debug, Default)]
struct Sessions {
	by_id: HashMap<String, Arc<Session>>,
	by_source: HashMap<PathBuf, String>,
}

impl PreviewManager {
	/// A manager that writes its segments under `base`, which is created on
	/// the first session and removed with the manager, so it must be a
	/// folder of its own. Previews of the same source file share a session.
	#[must_use]
	pub fn new(runner: Runner, config: PreviewConfig, base: PathBuf) -> Self {
		Self::with_root(runner, config, ScratchRoot::from(base))
	}

	/// A manager under the system temporary folder, in a subfolder with a
	/// random name made on the first session.
	#[must_use]
	pub fn in_temp_dir(runner: Runner, config: PreviewConfig) -> Self {
		Self::with_root(runner, config, ScratchRoot::temp("muxmultitool-preview-"))
	}

	fn with_root(runner: Runner, config: PreviewConfig, base: ScratchRoot) -> Self {
		Self {
			inner: Arc::new(Inner {
				runner,
				config,
				base,
				sessions: Mutex::default(),
			}),
		}
	}

	/// The configuration.
	#[must_use]
	pub fn config(&self) -> &PreviewConfig {
		&self.inner.config
	}

	/// Starts a session for `source`, or returns the one it already has.
	///
	/// # Errors
	/// See [`PreviewError`]. The caller has validated that `source` may be
	/// read at all; this only checks that it is a media file.
	pub async fn start(&self, source: &Path) -> Result<PreviewInfo, PreviewError> {
		let canonical = tokio::fs::canonicalize(source)
			.await
			.map_err(|source_err| PreviewError::Open {
				path: source.to_path_buf(),
				source: source_err,
			})?;
		let meta = tokio::fs::metadata(&canonical)
			.await
			.map_err(|source_err| PreviewError::Open {
				path: source.to_path_buf(),
				source: source_err,
			})?;
		if !meta.is_file() {
			return Err(PreviewError::NotAFile(source.to_path_buf()));
		}
		if let Some(session) = self.inner.by_source(&canonical) {
			session.touch();
			return Ok(session.info());
		}

		let inner = &self.inner;
		let limits = Limits::timeout(inner.config.scan_timeout);
		let probe = inner
			.runner
			.probe(&canonical, &limits)
			.await
			.map_err(PreviewError::Probe)?;
		let streams = Streams::pick(&probe).ok_or(PreviewError::NoStreams)?;
		let duration = probe
			.duration_secs()
			.filter(|d| *d > 0.0)
			.ok_or(PreviewError::NoDuration)?;
		let plan = inner.plan(&canonical, streams, duration).await;

		let id = new_id().map_err(PreviewError::Io)?;
		let dir = inner.base.path().map_err(PreviewError::Io)?.join(&id);
		tokio::fs::create_dir_all(&dir)
			.await
			.map_err(PreviewError::Io)?;
		let session = Arc::new(Session {
			id: id.clone(),
			source: canonical.clone(),
			dir,
			plan,
			runner: inner.runner.clone(),
			config: inner.config.clone(),
			state: Mutex::default(),
			changed: Notify::new(),
			last_access: Mutex::new(Instant::now()),
			runs: AtomicU64::new(0),
			expiry: Mutex::new(None),
		});

		// Another request for the same file may have won meanwhile.
		let session = {
			let mut sessions = inner.lock_sessions();
			if let Some(existing) = sessions
				.by_source
				.get(&canonical)
				.and_then(|id| sessions.by_id.get(id))
				.cloned()
			{
				existing
			} else {
				sessions.by_source.insert(canonical, id.clone());
				sessions.by_id.insert(id, Arc::clone(&session));
				session
			}
		};
		session.start_expiry(Arc::downgrade(inner));
		info!(
			id = session.id,
			source = %session.source.display(),
			mode = ?session.plan.mode(),
			segments = session.plan.playlist.len(),
			"preview session started"
		);
		Ok(session.info())
	}

	/// The playlist of a session as `index.m3u8` text.
	#[must_use]
	pub fn playlist(&self, id: &str) -> Option<String> {
		let session = self.inner.by_id(id)?;
		session.touch();
		Some(session.plan.playlist.to_m3u8())
	}

	/// What [`start`](Self::start) returned for a session.
	#[must_use]
	pub fn info(&self, id: &str) -> Option<PreviewInfo> {
		self.inner.by_id(id).map(|s| s.info())
	}

	/// The file of segment `n` of a session, producing it first if needed.
	///
	/// # Errors
	/// See [`SegmentError`].
	pub async fn segment(&self, id: &str, n: usize) -> Result<PathBuf, SegmentError> {
		let session = self
			.inner
			.by_id(id)
			.ok_or_else(|| SegmentError::UnknownSession(id.to_owned()))?;
		session.touch();
		session.segment(n).await
	}

	/// Whether a session has an ffmpeg run going. For tests and diagnostics.
	#[must_use]
	pub fn is_running(&self, id: &str) -> bool {
		self.inner
			.by_id(id)
			.is_some_and(|s| s.lock_state().run.as_ref().is_some_and(|r| !r.done))
	}

	/// The folder with a session's segments. For tests and diagnostics.
	#[must_use]
	pub fn session_dir(&self, id: &str) -> Option<PathBuf> {
		self.inner.by_id(id).map(|s| s.dir.clone())
	}

	/// Ends a session: stops ffmpeg and removes its segments. `false` when
	/// there was no such session.
	pub async fn stop(&self, id: &str) -> bool {
		self.inner.remove(id).await
	}

	/// Ends every session and removes the base folder.
	pub async fn shutdown(&self) {
		let ids: Vec<String> = self.inner.lock_sessions().by_id.keys().cloned().collect();
		for id in ids {
			self.inner.remove(&id).await;
		}
		if let Some(base) = self.inner.base.existing()
			&& let Err(error) = tokio::fs::remove_dir_all(&base).await
			&& error.kind() != io::ErrorKind::NotFound
		{
			warn!(dir = %base.display(), %error, "cannot remove the preview folder");
		}
	}
}

impl Inner {
	fn lock_sessions(&self) -> MutexGuard<'_, Sessions> {
		self.sessions
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	fn by_id(&self, id: &str) -> Option<Arc<Session>> {
		self.lock_sessions().by_id.get(id).cloned()
	}

	fn by_source(&self, source: &Path) -> Option<Arc<Session>> {
		let sessions = self.lock_sessions();
		sessions
			.by_source
			.get(source)
			.and_then(|id| sessions.by_id.get(id))
			.cloned()
	}

	/// Decides copy or transcode for `streams` and builds the playlist. A
	/// copy needs the source's keyframes; when they cannot be had or do not
	/// make usable segments, the video is transcoded on the fixed grid.
	async fn plan(&self, source: &Path, streams: Streams, duration: f64) -> Plan {
		let config = &self.config;
		let mut plan = Plan {
			video: streams.video,
			audio: streams.audio,
			video_copy: streams.video_copy,
			audio_copy: streams.audio_copy,
			downscale: streams.downscale,
			playlist: Playlist::grid(duration, config.segment_secs),
		};
		let Some(video) = streams.video.filter(|_| streams.video_copy) else {
			return plan;
		};
		let keyframes = self
			.runner
			.keyframes(source, video, &Limits::timeout(config.scan_timeout))
			.await;
		match keyframes {
			Ok(keyframes) => {
				if let Some(playlist) = Playlist::from_keyframes(
					&keyframes,
					duration,
					config.segment_secs,
					config.max_copy_segment_secs,
					KEYFRAME_GUARD_SECS,
				) {
					plan.playlist = playlist;
					return plan;
				}
				info!(
					source = %source.display(),
					keyframes = keyframes.len(),
					"keyframes do not make usable segments; transcoding"
				);
			}
			Err(error) => {
				warn!(source = %source.display(), %error, "keyframe scan failed; transcoding");
			}
		}
		plan.video_copy = false;
		plan
	}

	async fn remove(&self, id: &str) -> bool {
		let session = {
			let mut sessions = self.lock_sessions();
			let session = sessions.by_id.remove(id);
			if let Some(session) = &session {
				sessions.by_source.remove(&session.source);
			}
			session
		};
		let Some(session) = session else {
			return false;
		};
		session.close().await;
		info!(id, "preview session removed");
		true
	}
}

impl Drop for Inner {
	/// Stops every run and removes the base folder, as far as a drop can:
	/// the runner kills ffmpeg when a run's token is cancelled, but that
	/// happens on the runtime after this returns, so a run still writing
	/// may leave its folder behind. [`PreviewManager::shutdown`] is the
	/// clean way.
	fn drop(&mut self) {
		let sessions = self.lock_sessions();
		for session in sessions.by_id.values() {
			if let Some(run) = &session.lock_state().run {
				run.cancel.cancel();
			}
			if let Some(expiry) = session.lock_expiry().take() {
				expiry.abort();
			}
		}
		self.base.remove_on_drop();
	}
}

/// A session id: 128 random bits from the operating system, as the id is
/// the only thing that guards a session's URLs.
fn new_id() -> io::Result<String> {
	let mut bytes = [0u8; 16];
	getrandom::fill(&mut bytes).map_err(io::Error::other)?;
	Ok(format!("{:032x}", u128::from_be_bytes(bytes)))
}

/// One source file's preview.
#[derive(Debug)]
struct Session {
	id: String,
	source: PathBuf,
	/// This session's folder; each run has a subfolder.
	dir: PathBuf,
	plan: Plan,
	runner: Runner,
	config: PreviewConfig,
	state: Mutex<State>,
	/// Signalled whenever `state` changes in a way a waiting request cares about.
	changed: Notify,
	last_access: Mutex<Instant>,
	runs: AtomicU64,
	expiry: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Debug, Default)]
struct State {
	/// Complete segment files by segment number, from any run.
	ready: HashMap<usize, PathBuf>,
	/// The current run, if one was started.
	run: Option<Run>,
	/// A run reached the end of the file: segments from here on cannot be made.
	unavailable_from: Option<usize>,
	/// A run failed from this segment on, with ffmpeg's log tail.
	failure: Option<(usize, String)>,
}

#[derive(Debug)]
struct Run {
	id: u64,
	start: usize,
	/// The segment the run is producing now (the last complete one plus one).
	next: usize,
	cancel: CancellationToken,
	task: Option<JoinHandle<()>>,
	done: bool,
}

#[derive(Debug)]
enum Outcome {
	/// ffmpeg reached the end of the file.
	Completed,
	/// The run was cancelled: a seek, idleness or a stop.
	Stopped,
	/// ffmpeg failed.
	Failed(String),
}

impl Session {
	fn lock_state(&self) -> MutexGuard<'_, State> {
		self.state
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	fn lock_expiry(&self) -> MutexGuard<'_, Option<JoinHandle<()>>> {
		self.expiry
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	fn info(&self) -> PreviewInfo {
		PreviewInfo {
			id: self.id.clone(),
			mode: self.plan.mode(),
			duration: self.plan.playlist.duration(),
			segment_count: self.plan.playlist.len(),
		}
	}

	fn touch(&self) {
		*self
			.last_access
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
	}

	fn idle_for(&self) -> Duration {
		self.last_access
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.elapsed()
	}

	/// Waits for segment `n`, starting a run when none will produce it.
	async fn segment(self: &Arc<Self>, n: usize) -> Result<PathBuf, SegmentError> {
		if n >= self.plan.playlist.len() {
			return Err(SegmentError::OutOfRange(n));
		}
		let timeout = self.config.wait_timeout;
		let deadline = Instant::now() + timeout;
		loop {
			// Registered before the state is read, so a change between the
			// read and the await is not missed.
			let notified = self.changed.notified();
			tokio::pin!(notified);
			notified.as_mut().enable();
			{
				let mut state = self.lock_state();
				if let Some(path) = state.ready.get(&n) {
					return Ok(path.clone());
				}
				if let Some((from, log)) = &state.failure
					&& n >= *from
				{
					return Err(SegmentError::Failed(log.clone()));
				}
				if state.unavailable_from.is_some_and(|from| n >= from) {
					return Err(SegmentError::NotProduced(n));
				}
				let covered = state.run.as_ref().is_some_and(|run| {
					!run.done && run.start <= n && n < run.next + self.config.lookahead
				});
				if !covered {
					self.start_run(&mut state, n);
				}
			}
			let remaining = deadline.saturating_duration_since(Instant::now());
			if remaining.is_zero() || tokio::time::timeout(remaining, notified).await.is_err() {
				return Err(SegmentError::Timeout { n, timeout });
			}
		}
	}

	/// Replaces the current run with one starting at `start`. The old run's
	/// task finishes by itself once ffmpeg is gone.
	fn start_run(self: &Arc<Self>, state: &mut State, start: usize) {
		if let Some(old) = state.run.take() {
			old.cancel.cancel();
		}
		let id = self.runs.fetch_add(1, Ordering::Relaxed) + 1;
		let cancel = CancellationToken::new();
		let task = tokio::spawn(run_task(Arc::clone(self), id, start, cancel.clone()));
		state.run = Some(Run {
			id,
			start,
			next: start,
			cancel,
			task: Some(task),
			done: false,
		});
	}

	/// Reads the segment list a run has written so far and marks the listed
	/// segments ready. Only whole lines count: ffmpeg may be mid-write.
	async fn absorb_list(&self, run_id: u64, dir: &Path, list: &Path) {
		let Ok(text) = tokio::fs::read_to_string(list).await else {
			return;
		};
		let complete = text.rsplit_once('\n').map_or("", |(lines, _)| lines);
		let found: Vec<(usize, PathBuf)> = complete
			.lines()
			.filter_map(|line| {
				let name = line.split(',').next()?;
				let n = name.strip_suffix(".ts")?.parse().ok()?;
				Some((n, dir.join(name)))
			})
			.collect();
		if found.is_empty() {
			return;
		}
		let mut state = self.lock_state();
		let mut changed = false;
		for (n, path) in found {
			if let Some(run) = state.run.as_mut().filter(|run| run.id == run_id) {
				run.next = run.next.max(n + 1);
			}
			if state.ready.insert(n, path).is_none() {
				changed = true;
			}
		}
		if changed {
			self.changed.notify_waiters();
		}
	}

	fn finish_run(&self, run_id: u64, outcome: Outcome) {
		let mut state = self.lock_state();
		let Some(run) = state.run.as_mut().filter(|run| run.id == run_id) else {
			// Superseded by a newer run; nothing to record.
			return;
		};
		run.done = true;
		let (start, next) = (run.start, run.next);
		match outcome {
			Outcome::Completed => {
				state.unavailable_from = Some(state.unavailable_from.map_or(next, |u| u.min(next)));
			}
			Outcome::Stopped => {}
			Outcome::Failed(log) => state.failure = Some((start, log)),
		}
		self.changed.notify_waiters();
	}

	/// Removes the session when it has been idle for the configured time.
	fn start_expiry(self: &Arc<Self>, manager: Weak<Inner>) {
		let session = Arc::downgrade(self);
		let task = tokio::spawn(async move {
			loop {
				let Some(strong) = session.upgrade() else {
					return;
				};
				let (idle, ttl, id) = (
					strong.idle_for(),
					strong.config.session_idle,
					strong.id.clone(),
				);
				drop(strong);
				if idle >= ttl {
					if let Some(manager) = manager.upgrade() {
						info!(id, "preview session expired");
						manager.remove(&id).await;
					}
					return;
				}
				tokio::time::sleep(ttl.saturating_sub(idle)).await;
			}
		});
		*self.lock_expiry() = Some(task);
	}

	/// Stops the run and deletes the folder.
	async fn close(&self) {
		if let Some(expiry) = self.lock_expiry().take()
			&& tokio::task::try_id() != Some(expiry.id())
		{
			expiry.abort();
		}
		let run = self.lock_state().run.take();
		if let Some(mut run) = run {
			run.cancel.cancel();
			if let Some(task) = run.task.take()
				&& tokio::time::timeout(STOP_GRACE, task).await.is_err()
			{
				warn!(id = self.id, "ffmpeg did not stop in time");
			}
		}
		if let Err(error) = tokio::fs::remove_dir_all(&self.dir).await
			&& error.kind() != io::ErrorKind::NotFound
		{
			warn!(id = self.id, %error, "cannot remove the session folder");
		}
	}
}

/// One ffmpeg process producing segments from `start` on, until the file
/// ends, the run is cancelled or nobody asks for segments any more.
async fn run_task(session: Arc<Session>, run_id: u64, start: usize, cancel: CancellationToken) {
	let dir = session.dir.join(format!("run-{run_id}"));
	if let Err(error) = tokio::fs::create_dir_all(&dir).await {
		session.finish_run(
			run_id,
			Outcome::Failed(format!("cannot create {}: {error}", dir.display())),
		);
		return;
	}
	let list = dir.join("segments.csv");
	let cmd = command::run_command(&session.source, &session.plan, start, &dir, &list);
	let limits = Limits::cancellable(cancel.clone());
	let runner = session.runner.clone();
	let ffmpeg = runner.run(&cmd, &limits, None);
	tokio::pin!(ffmpeg);
	let mut poll = tokio::time::interval(RUN_POLL);
	poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
	let result = loop {
		tokio::select! {
			result = &mut ffmpeg => break result,
			_ = poll.tick() => {
				session.absorb_list(run_id, &dir, &list).await;
				if !cancel.is_cancelled() && session.idle_for() >= session.config.run_idle {
					info!(id = session.id, run = run_id, "no segment requests; stopping ffmpeg");
					cancel.cancel();
				}
			}
		}
	};
	session.absorb_list(run_id, &dir, &list).await;
	let outcome = match result {
		Ok(_) => Outcome::Completed,
		Err(RunError::Cancelled { .. }) => Outcome::Stopped,
		Err(error) => {
			warn!(id = session.id, run = run_id, %error, "preview run failed");
			Outcome::Failed(error.to_string())
		}
	};
	session.finish_run(run_id, outcome);
}
