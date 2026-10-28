//! Timeline analysis of source files: thumbnail sprites, waveform peaks
//! and subtitle cues for the cut editor.
//!
//! Each analysis runs once per file (and stream) in the background, as one
//! ffmpeg process, and streams its results to anyone watching while it
//! runs: a [`TimelineManager`] hands out a *watch* that yields what is
//! known now and every addition until the analysis ends. Results stay in
//! memory (peaks, cues) or on disk (sprites) and are served again without
//! ffmpeg, until the file changes: every request compares the file's size
//! and modification time with the cached entry, and a change starts over
//! under a new id, so a browser cannot mix old sprites with new ones.
//!
//! - [`thumbs`]: sprite images on disk, served by the HTTP media route
//!   through [`TimelineManager::sprite`];
//! - [`peaks`]: base-resolution peaks in memory, downsampled per request;
//! - [`cues`]: text cues in memory.
//!
//! A watch ends with an [`AnalysisError`] when ffmpeg fails or when the
//! analysis is dropped: the file changed, the entry was evicted, or the
//! manager shut down.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::ffmpeg::probe::{CodecType, ProbeResult, Rational, Stream};
use crate::ffmpeg::{FfmpegCommand, Limits, RunError, Runner};
use crate::fs::ScratchRoot;

pub mod cues;
pub mod peaks;
pub mod thumbs;

pub use cues::Cue;
pub use peaks::Peak;
pub use thumbs::Sheet;

/// How often a running analysis looks for new output and wakes watchers.
const POLL: Duration = Duration::from_millis(150);

/// How long closing an analysis waits for its ffmpeg to go away.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Buffered stderr lines between the runner and an analysis.
const LOG_CHANNEL: usize = 1024;

/// Tunables of the analyses. [`Default`] is what the server uses.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineConfig {
	/// Width of a thumbnail tile in pixels; the height follows the picture.
	pub tile_width: u32,
	/// Tiles per sprite row.
	pub columns: u32,
	/// Rows per sprite.
	pub rows: u32,
	/// Most tiles for one file; longer files get a longer interval.
	pub max_tiles: u32,
	/// Shortest interval between two tiles, for short files.
	pub min_interval_secs: f64,
	/// Files with more frames than this are decoded keyframes-only for the
	/// thumbnails; see [`thumbs`].
	pub full_decode_max_frames: f64,
	/// Base resolution of the waveform: peaks measured per second of audio.
	pub peaks_per_second: u32,
	/// Most peaks one waveform request may ask for.
	pub max_peaks: usize,
	/// Files kept in the cache; the least recently used is dropped beyond it.
	pub max_files: usize,
	/// Limit for one analysis run of ffmpeg.
	pub analysis_timeout: Duration,
	/// Limit for the probe when a file is first seen.
	pub probe_timeout: Duration,
}

impl Default for TimelineConfig {
	fn default() -> Self {
		Self {
			tile_width: 160,
			columns: 10,
			rows: 10,
			max_tiles: 1200,
			min_interval_secs: 0.5,
			full_decode_max_frames: 36_000.0,
			peaks_per_second: 50,
			max_peaks: 8192,
			max_files: 16,
			analysis_timeout: Duration::from_secs(60 * 60),
			probe_timeout: Duration::from_secs(120),
		}
	}
}

/// Why an analysis could not be started.
#[derive(Debug, thiserror::Error)]
pub enum TimelineError {
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
	/// Without a duration there is no timeline.
	#[error("the file has no known duration")]
	NoDuration,
	/// Thumbnails need a video stream.
	#[error("the file has no video stream")]
	NoVideo,
	/// The file has no stream with that index.
	#[error("the file has no stream {0}")]
	NoStream(u32),
	/// A waveform needs an audio stream.
	#[error("stream {0} is not an audio stream")]
	NotAudio(u32),
	/// Cues need a subtitle stream.
	#[error("stream {0} is not a subtitle stream")]
	NotSubtitle(u32),
	/// The subtitles are pictures; see [`cues::is_image_codec`].
	#[error("stream {index} is an image-based subtitle ({codec}), which has no text to show")]
	ImageSubtitles {
		/// The stream.
		index: u32,
		/// Its codec name.
		codec: String,
	},
	/// The waveform window makes no sense.
	#[error("invalid window: {0}")]
	InvalidWindow(String),
	/// The cache folder could not be created.
	#[error("cannot create the cache folder: {0}")]
	Io(#[source] io::Error),
}

/// Why an analysis ended without its result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisError {
	/// ffmpeg failed; the text is the error with the tail of its log.
	#[error("analysis failed: {0}")]
	Failed(String),
	/// The analysis was dropped: the file changed, its entry was evicted or
	/// the server is stopping.
	#[error("the analysis was cancelled")]
	Cancelled,
}

/// What a thumbnail watch yields: the sheet and how many sprites exist.
#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailUpdate {
	/// The `{id}` of the sprite route.
	pub media_id: String,
	/// The layout; the real tile and sprite counts once `done`.
	pub sheet: Sheet,
	/// Sprites `0..ready` can be fetched.
	pub ready: usize,
	/// Whether this is the last update.
	pub done: bool,
}

/// A time window at a resolution, as served.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
	/// Start in seconds.
	pub from: f64,
	/// End in seconds.
	pub to: f64,
	/// Number of peaks over the window.
	pub count: usize,
}

impl Window {
	/// Seconds per peak.
	#[must_use]
	pub fn bin_width(&self) -> f64 {
		#[expect(clippy::cast_precision_loss, reason = "counts are small")]
		let count = self.count.max(1) as f64;
		(self.to - self.from) / count
	}
}

/// What a waveform watch yields: the next peaks of the window.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveformUpdate {
	/// The window as served.
	pub window: Window,
	/// Index of the first peak in `peaks` within the window.
	pub offset: usize,
	/// The next peaks.
	pub peaks: Vec<Peak>,
	/// How far the analysis of the stream got, in seconds.
	pub analyzed_secs: f64,
	/// Whether every peak of the window has been sent.
	pub done: bool,
}

/// What a cue watch yields: the next cues.
#[derive(Debug, Clone, PartialEq)]
pub struct CueUpdate {
	/// The next cues, in file order.
	pub cues: Vec<Cue>,
	/// Whether this is the last update.
	pub done: bool,
}

/// The analyses of one server. Cheap to clone; clones share the cache.
#[derive(Debug, Clone)]
pub struct TimelineManager {
	inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
	runner: Runner,
	config: TimelineConfig,
	/// Folder with one subfolder per cached file.
	base: ScratchRoot,
	entries: Mutex<Entries>,
}

#[derive(Debug, Default)]
struct Entries {
	by_id: HashMap<String, Arc<Entry>>,
	by_source: HashMap<PathBuf, String>,
}

impl Entries {
	fn remove(&mut self, id: &str) -> Option<Arc<Entry>> {
		let entry = self.by_id.remove(id)?;
		self.by_source.remove(&entry.source);
		Some(entry)
	}
}

impl TimelineManager {
	/// A manager that keeps its sprites under `base`, which is created on
	/// the first file and removed with the manager, so it must be a folder
	/// of its own.
	#[must_use]
	pub fn new(runner: Runner, config: TimelineConfig, base: PathBuf) -> Self {
		Self::with_root(runner, config, ScratchRoot::from(base))
	}

	/// A manager under the system temporary folder, in a subfolder with a
	/// random name made on the first file.
	#[must_use]
	pub fn in_temp_dir(runner: Runner, config: TimelineConfig) -> Self {
		Self::with_root(runner, config, ScratchRoot::temp("muxmultitool-timeline-"))
	}

	fn with_root(runner: Runner, config: TimelineConfig, base: ScratchRoot) -> Self {
		Self {
			inner: Arc::new(Inner {
				runner,
				config,
				base,
				entries: Mutex::default(),
			}),
		}
	}

	/// The configuration.
	#[must_use]
	pub fn config(&self) -> &TimelineConfig {
		&self.inner.config
	}

	/// The thumbnails of `source`, generating them in the background on the
	/// first call.
	///
	/// # Errors
	/// See [`TimelineError`]. The caller has validated that `source` may be
	/// read at all; this only checks that it is a video file.
	pub async fn thumbnails(&self, source: &Path) -> Result<ThumbnailWatch, TimelineError> {
		let entry = self.inner.entry(source).await?;
		let video = entry
			.probe
			.streams_of(CodecType::Video)
			.find(|s| !s.disposition.attached_pic)
			.ok_or(TimelineError::NoVideo)?;
		let analysis = {
			let mut slots = entry.lock_analyses();
			if let Some(existing) = &slots.thumbs {
				Arc::clone(existing)
			} else {
				let sheet = Sheet::plan(
					entry.duration,
					video.width.unwrap_or(0),
					video.height.unwrap_or(0),
					video
						.display_aspect_ratio
						.as_deref()
						.and_then(|dar| dar.parse::<Rational>().ok())
						.and_then(Rational::as_f64),
					video.frame_rate(),
					&self.inner.config,
				);
				let dir = entry.dir.join("thumbs");
				let cmd = thumbs::command(&entry.source, video.index, &sheet, &dir);
				let analysis = Analysis::new(Thumbs { sheet, ready: 0 });
				analysis.spawn(thumbs_task(
					Arc::clone(&analysis),
					self.inner.runner.clone(),
					cmd,
					dir,
					self.inner.config.analysis_timeout,
				));
				info!(id = entry.id, source = %entry.source.display(), "thumbnail generation started");
				slots.thumbs = Some(Arc::clone(&analysis));
				analysis
			}
		};
		Ok(ThumbnailWatch {
			media_id: entry.id.clone(),
			analysis,
			sent: None,
			finished: false,
		})
	}

	/// The waveform of audio stream `stream_index` of `source` over
	/// `from..to` seconds (`to` at most the duration; `0` or more means the
	/// end) in `count` peaks, analysing the stream in the background on the
	/// first call.
	///
	/// # Errors
	/// See [`TimelineError`].
	pub async fn waveform(
		&self,
		source: &Path,
		stream_index: u32,
		from: f64,
		to: f64,
		count: usize,
	) -> Result<WaveformWatch, TimelineError> {
		let entry = self.inner.entry(source).await?;
		let stream = entry.stream(stream_index)?;
		if stream.codec_type != CodecType::Audio {
			return Err(TimelineError::NotAudio(stream_index));
		}
		let window = self.window(from, to, count, entry.duration)?;
		let analysis = {
			let mut slots = entry.lock_analyses();
			if let Some(existing) = slots.peaks.get(&stream_index) {
				Arc::clone(existing)
			} else {
				let sample_rate = stream.sample_rate.filter(|r| *r > 0).unwrap_or(48_000);
				let samples_per_bin =
					(sample_rate / self.inner.config.peaks_per_second.max(1)).max(1);
				let bin_secs = f64::from(samples_per_bin) / f64::from(sample_rate);
				let cmd = peaks::command(&entry.source, stream_index, sample_rate, samples_per_bin);
				let analysis = Analysis::new(Peaks {
					bin_secs,
					peaks: Vec::new(),
				});
				analysis.spawn(peaks_task(
					Arc::clone(&analysis),
					self.inner.runner.clone(),
					cmd,
					self.inner.config.analysis_timeout,
				));
				info!(
					id = entry.id,
					stream = stream_index,
					"waveform analysis started"
				);
				slots.peaks.insert(stream_index, Arc::clone(&analysis));
				analysis
			}
		};
		Ok(WaveformWatch {
			analysis,
			window,
			sent: 0,
			started: false,
			finished: false,
		})
	}

	/// The cues of text subtitle stream `stream_index` of `source`, reading
	/// them in the background on the first call.
	///
	/// # Errors
	/// See [`TimelineError`]; image-based subtitles are
	/// [`TimelineError::ImageSubtitles`].
	pub async fn subtitle_cues(
		&self,
		source: &Path,
		stream_index: u32,
	) -> Result<CueWatch, TimelineError> {
		let entry = self.inner.entry(source).await?;
		check_subtitle(entry.stream(stream_index)?)?;
		let analysis = {
			let mut slots = entry.lock_analyses();
			if let Some(existing) = slots.cues.get(&stream_index) {
				Arc::clone(existing)
			} else {
				let output = entry.dir.join(format!("cues-{stream_index}.vtt"));
				let cmd = cues::command(&entry.source, stream_index, &output);
				let analysis = Analysis::new(Cues::default());
				analysis.spawn(cues_task(
					Arc::clone(&analysis),
					self.inner.runner.clone(),
					cmd,
					output,
					self.inner.config.analysis_timeout,
				));
				info!(
					id = entry.id,
					stream = stream_index,
					"subtitle reading started"
				);
				slots.cues.insert(stream_index, Arc::clone(&analysis));
				analysis
			}
		};
		Ok(CueWatch {
			analysis,
			sent: 0,
			started: false,
			finished: false,
		})
	}

	/// The file of sprite `n` of the cached file `id`, when it is ready.
	#[must_use]
	pub fn sprite(&self, id: &str, n: usize) -> Option<PathBuf> {
		let entry = self.inner.by_id(id)?;
		entry.touch();
		let thumbs = entry.lock_analyses().thumbs.clone()?;
		let ready = thumbs.lock().0.ready;
		(n < ready).then(|| entry.dir.join("thumbs").join(format!("{n}.jpg")))
	}

	/// Drops the cached results of `source`, stopping any analysis. `false`
	/// when nothing was cached for it.
	pub async fn forget(&self, source: &Path) -> bool {
		let Ok(canonical) = tokio::fs::canonicalize(source).await else {
			return false;
		};
		let entry = {
			let mut entries = self.inner.lock_entries();
			let id = entries.by_source.get(&canonical).cloned();
			id.and_then(|id| entries.remove(&id))
		};
		match entry {
			Some(entry) => {
				entry.close().await;
				true
			}
			None => false,
		}
	}

	/// Stops every analysis and removes the cache folder.
	pub async fn shutdown(&self) {
		let entries: Vec<Arc<Entry>> = {
			let mut entries = self.inner.lock_entries();
			entries.by_source.clear();
			entries.by_id.drain().map(|(_, entry)| entry).collect()
		};
		for entry in entries {
			entry.close().await;
		}
		if let Some(base) = self.inner.base.existing()
			&& let Err(error) = tokio::fs::remove_dir_all(&base).await
			&& error.kind() != io::ErrorKind::NotFound
		{
			warn!(dir = %base.display(), %error, "cannot remove the timeline folder");
		}
	}

	/// Validates and clamps a waveform window.
	fn window(
		&self,
		from: f64,
		to: f64,
		count: usize,
		duration: f64,
	) -> Result<Window, TimelineError> {
		if !from.is_finite() || from < 0.0 {
			return Err(TimelineError::InvalidWindow(format!(
				"from must be 0 or more, not {from}"
			)));
		}
		let to = if to.is_finite() && to > 0.0 {
			to.min(duration)
		} else {
			duration
		};
		if from >= to {
			return Err(TimelineError::InvalidWindow(format!(
				"from ({from}) must be before to ({to})"
			)));
		}
		let max = self.inner.config.max_peaks;
		if count == 0 || count > max {
			return Err(TimelineError::InvalidWindow(format!(
				"peak count must be 1 to {max}, not {count}"
			)));
		}
		Ok(Window { from, to, count })
	}
}

impl Inner {
	fn lock_entries(&self) -> MutexGuard<'_, Entries> {
		self.entries
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	fn by_id(&self, id: &str) -> Option<Arc<Entry>> {
		self.lock_entries().by_id.get(id).cloned()
	}

	/// The cached entry of `source`, probing the file when it is new or has
	/// changed since it was cached.
	async fn entry(&self, source: &Path) -> Result<Arc<Entry>, TimelineError> {
		let open_error = |source_err| TimelineError::Open {
			path: source.to_path_buf(),
			source: source_err,
		};
		let canonical = tokio::fs::canonicalize(source).await.map_err(open_error)?;
		let meta = tokio::fs::metadata(&canonical).await.map_err(open_error)?;
		if !meta.is_file() {
			return Err(TimelineError::NotAFile(source.to_path_buf()));
		}
		let fingerprint = Fingerprint {
			len: meta.len(),
			modified: meta.modified().ok(),
		};

		let stale = {
			let mut entries = self.lock_entries();
			let cached = entries
				.by_source
				.get(&canonical)
				.and_then(|id| entries.by_id.get(id))
				.cloned();
			match cached {
				Some(entry) if entry.fingerprint == fingerprint => {
					entry.touch();
					return Ok(entry);
				}
				Some(entry) => entries.remove(&entry.id),
				None => None,
			}
		};
		if let Some(stale) = stale {
			info!(id = stale.id, source = %canonical.display(), "file changed; dropping its timeline cache");
			stale.close().await;
		}

		let probe = self
			.runner
			.probe(&canonical, &Limits::timeout(self.config.probe_timeout))
			.await
			.map_err(TimelineError::Probe)?;
		let duration = probe
			.duration_secs()
			.filter(|d| *d > 0.0)
			.ok_or(TimelineError::NoDuration)?;
		let id = new_id().map_err(TimelineError::Io)?;
		let dir = self.base.path().map_err(TimelineError::Io)?.join(&id);
		tokio::fs::create_dir_all(&dir)
			.await
			.map_err(TimelineError::Io)?;
		let entry = Arc::new(Entry {
			id: id.clone(),
			source: canonical.clone(),
			fingerprint,
			dir,
			probe,
			duration,
			analyses: Mutex::default(),
			last_access: Mutex::new(Instant::now()),
		});

		// Another request for the same file may have won meanwhile; and the
		// cache has a size.
		let (entry, evicted) = {
			let mut entries = self.lock_entries();
			if let Some(existing) = entries
				.by_source
				.get(&canonical)
				.and_then(|id| entries.by_id.get(id))
				.cloned()
			{
				(existing, vec![entry])
			} else {
				entries.by_source.insert(canonical, id.clone());
				entries.by_id.insert(id, Arc::clone(&entry));
				let mut evicted = Vec::new();
				while entries.by_id.len() > self.config.max_files.max(1) {
					let oldest = entries
						.by_id
						.values()
						.filter(|e| e.id != entry.id)
						.min_by_key(|e| e.last_access_at())
						.map(|e| e.id.clone());
					match oldest.and_then(|id| entries.remove(&id)) {
						Some(old) => evicted.push(old),
						None => break,
					}
				}
				(entry, evicted)
			}
		};
		for old in evicted {
			info!(id = old.id, source = %old.source.display(), "timeline cache entry dropped");
			old.close().await;
		}
		Ok(entry)
	}
}

impl Drop for Inner {
	/// Cancels every analysis and removes the cache folder, as far as a drop
	/// can: a run still writing may leave its folder behind.
	/// [`TimelineManager::shutdown`] is the clean way.
	fn drop(&mut self) {
		for entry in self.lock_entries().by_id.values() {
			entry.cancel_all();
		}
		self.base.remove_on_drop();
	}
}

/// An entry id: 128 random bits from the operating system, as the id is
/// the only thing that guards an entry's sprite URLs.
fn new_id() -> io::Result<String> {
	let mut bytes = [0u8; 16];
	getrandom::fill(&mut bytes).map_err(io::Error::other)?;
	Ok(format!("{:032x}", u128::from_be_bytes(bytes)))
}

/// What identifies a file's contents without reading them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
	len: u64,
	modified: Option<SystemTime>,
}

/// One cached file.
#[derive(Debug)]
struct Entry {
	id: String,
	source: PathBuf,
	fingerprint: Fingerprint,
	/// This entry's folder: sprites and scratch files.
	dir: PathBuf,
	probe: ProbeResult,
	duration: f64,
	analyses: Mutex<Analyses>,
	last_access: Mutex<Instant>,
}

#[derive(Debug, Default)]
struct Analyses {
	thumbs: Option<Arc<Analysis<Thumbs>>>,
	peaks: HashMap<u32, Arc<Analysis<Peaks>>>,
	cues: HashMap<u32, Arc<Analysis<Cues>>>,
}

impl Entry {
	fn lock_analyses(&self) -> MutexGuard<'_, Analyses> {
		self.analyses
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	fn stream(&self, index: u32) -> Result<&Stream, TimelineError> {
		self.probe
			.streams
			.iter()
			.find(|s| s.index == index)
			.ok_or(TimelineError::NoStream(index))
	}

	fn touch(&self) {
		*self
			.last_access
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
	}

	fn last_access_at(&self) -> Instant {
		*self
			.last_access
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	fn cancel_all(&self) {
		let analyses = self.lock_analyses();
		if let Some(thumbs) = &analyses.thumbs {
			thumbs.cancel.cancel();
		}
		for analysis in analyses.peaks.values() {
			analysis.cancel.cancel();
		}
		for analysis in analyses.cues.values() {
			analysis.cancel.cancel();
		}
	}

	/// Stops every analysis and deletes the folder.
	async fn close(&self) {
		let (thumbs, peaks, cues) = {
			let mut analyses = self.lock_analyses();
			(
				analyses.thumbs.take(),
				analyses.peaks.drain().map(|(_, a)| a).collect::<Vec<_>>(),
				analyses.cues.drain().map(|(_, a)| a).collect::<Vec<_>>(),
			)
		};
		if let Some(thumbs) = thumbs {
			thumbs.close().await;
		}
		for analysis in peaks {
			analysis.close().await;
		}
		for analysis in cues {
			analysis.close().await;
		}
		if let Err(error) = tokio::fs::remove_dir_all(&self.dir).await
			&& error.kind() != io::ErrorKind::NotFound
		{
			warn!(id = self.id, %error, "cannot remove the timeline folder");
		}
	}
}

/// Where an analysis is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
	Running,
	Done,
	Failed(String),
	Cancelled,
}

impl Status {
	fn ended(&self) -> bool {
		*self != Self::Running
	}

	/// The error of an ended analysis that has no result.
	fn error(&self) -> Option<AnalysisError> {
		match self {
			Self::Running | Self::Done => None,
			Self::Failed(message) => Some(AnalysisError::Failed(message.clone())),
			Self::Cancelled => Some(AnalysisError::Cancelled),
		}
	}
}

/// One background analysis: its growing result, its status and the task
/// producing it. Watchers wait on `changed`.
#[derive(Debug)]
struct Analysis<T> {
	state: Mutex<(T, Status)>,
	changed: Notify,
	cancel: CancellationToken,
	task: Mutex<Option<JoinHandle<()>>>,
}

impl<T: Send + 'static> Analysis<T> {
	fn new(data: T) -> Arc<Self> {
		Arc::new(Self {
			state: Mutex::new((data, Status::Running)),
			changed: Notify::new(),
			cancel: CancellationToken::new(),
			task: Mutex::new(None),
		})
	}

	fn lock(&self) -> MutexGuard<'_, (T, Status)> {
		self.state
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
		*self
			.task
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tokio::spawn(task));
	}

	/// Runs `check` on the state now and after every change, until it
	/// returns a value.
	async fn wait<R>(&self, mut check: impl FnMut(&T, &Status) -> Option<R>) -> R {
		loop {
			// Registered before the state is read, so a change between the
			// read and the await is not missed.
			let notified = self.changed.notified();
			tokio::pin!(notified);
			notified.as_mut().enable();
			{
				let state = self.lock();
				if let Some(result) = check(&state.0, &state.1) {
					return result;
				}
			}
			notified.await;
		}
	}

	/// Changes the result; `update` says whether anything changed.
	fn update(&self, update: impl FnOnce(&mut T) -> bool) {
		let changed = update(&mut self.lock().0);
		if changed {
			self.changed.notify_waiters();
		}
	}

	fn finish(&self, status: Status) {
		self.lock().1 = status;
		self.changed.notify_waiters();
	}

	/// Stops ffmpeg and waits for the task.
	async fn close(&self) {
		self.cancel.cancel();
		let task = self
			.task
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.take();
		if let Some(task) = task
			&& tokio::task::try_id() != Some(task.id())
			&& tokio::time::timeout(STOP_GRACE, task).await.is_err()
		{
			warn!("an analysis did not stop in time");
		}
	}
}

/// The result of a thumbnail analysis.
#[derive(Debug)]
struct Thumbs {
	sheet: Sheet,
	/// Sprites `0..ready` are complete files.
	ready: usize,
}

/// The result of a waveform analysis.
#[derive(Debug)]
struct Peaks {
	bin_secs: f64,
	peaks: Vec<Peak>,
}

impl Peaks {
	fn analyzed_secs(&self) -> f64 {
		#[expect(clippy::cast_precision_loss, reason = "bin counts are small")]
		let bins = self.peaks.len() as f64;
		bins * self.bin_secs
	}
}

/// The result of a subtitle analysis.
#[derive(Debug, Default)]
struct Cues {
	cues: Vec<Cue>,
}

/// Follows a thumbnail analysis. See [`TimelineManager::thumbnails`].
#[derive(Debug)]
pub struct ThumbnailWatch {
	media_id: String,
	analysis: Arc<Analysis<Thumbs>>,
	/// The `ready` count last yielded.
	sent: Option<usize>,
	finished: bool,
}

impl ThumbnailWatch {
	/// The next update: the state now on the first call, then one per
	/// change, the last with `done`. `None` after that, or after an error.
	pub async fn next(&mut self) -> Option<Result<ThumbnailUpdate, AnalysisError>> {
		if self.finished {
			return None;
		}
		let Self {
			media_id,
			analysis,
			sent,
			finished,
		} = self;
		let result = analysis
			.wait(|thumbs, status| {
				if sent.is_none_or(|s| thumbs.ready > s) || status.ended() {
					Some(status.error().map_or_else(
						|| {
							Ok(ThumbnailUpdate {
								media_id: media_id.clone(),
								sheet: thumbs.sheet.clone(),
								ready: thumbs.ready,
								done: *status == Status::Done,
							})
						},
						Err,
					))
				} else {
					None
				}
			})
			.await;
		match &result {
			Ok(update) => {
				*sent = Some(update.ready);
				*finished = update.done;
			}
			Err(_) => *finished = true,
		}
		Some(result)
	}
}

/// Follows a waveform analysis for one window. See [`TimelineManager::waveform`].
#[derive(Debug)]
pub struct WaveformWatch {
	analysis: Arc<Analysis<Peaks>>,
	window: Window,
	/// Peaks `0..sent` of the window were yielded.
	sent: usize,
	started: bool,
	finished: bool,
}

impl WaveformWatch {
	/// The window as served.
	#[must_use]
	pub fn window(&self) -> Window {
		self.window
	}

	/// The next peaks: the ones the analysis has covered since the last
	/// call, the last update with `done`. `None` after that, or after an
	/// error. The first call answers at once, possibly with no peaks.
	pub async fn next(&mut self) -> Option<Result<WaveformUpdate, AnalysisError>> {
		if self.finished {
			return None;
		}
		let Self {
			analysis,
			window,
			sent,
			started,
			finished,
		} = self;
		let width = window.bin_width();
		let result = analysis
			.wait(|peaks, status| {
				let analyzed = peaks.analyzed_secs();
				let complete = if status.ended() {
					window.count
				} else {
					complete_bins(analyzed, window.from, width, window.count)
				};
				if complete > *sent || !*started || status.ended() {
					Some(status.error().map_or_else(
						|| {
							Ok(WaveformUpdate {
								window: *window,
								offset: *sent,
								peaks: peaks::downsample(
									&peaks.peaks,
									peaks.bin_secs,
									window.from,
									width,
									*sent..complete,
								),
								analyzed_secs: analyzed,
								done: complete >= window.count,
							})
						},
						Err,
					))
				} else {
					None
				}
			})
			.await;
		*started = true;
		match &result {
			Ok(update) => {
				*sent = update.offset + update.peaks.len();
				*finished = update.done;
			}
			Err(_) => *finished = true,
		}
		Some(result)
	}
}

/// Whether `stream` is a subtitle stream whose cues can be shown.
fn check_subtitle(stream: &Stream) -> Result<(), TimelineError> {
	if stream.codec_type != CodecType::Subtitle {
		return Err(TimelineError::NotSubtitle(stream.index));
	}
	let codec = stream.codec_name.clone().unwrap_or_default();
	if cues::is_image_codec(&codec) {
		return Err(TimelineError::ImageSubtitles {
			index: stream.index,
			codec,
		});
	}
	Ok(())
}

/// How many bins of a window starting at `from` with `width` seconds each
/// are fully covered by `analyzed` seconds of data, at most `count`.
fn complete_bins(analyzed: f64, from: f64, width: f64, count: usize) -> usize {
	if width <= 0.0 {
		return count;
	}
	let bins = ((analyzed - from) / width).floor();
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		clippy::cast_precision_loss,
		reason = "clamped to 0..=count first"
	)]
	let bins = bins.clamp(0.0, count as f64) as usize;
	bins.min(count)
}

/// Follows a subtitle analysis. See [`TimelineManager::subtitle_cues`].
#[derive(Debug)]
pub struct CueWatch {
	analysis: Arc<Analysis<Cues>>,
	/// Cues `0..sent` were yielded.
	sent: usize,
	started: bool,
	finished: bool,
}

impl CueWatch {
	/// The next cues, the last update with `done`. `None` after that, or
	/// after an error. The first call answers at once, possibly with no cues.
	pub async fn next(&mut self) -> Option<Result<CueUpdate, AnalysisError>> {
		if self.finished {
			return None;
		}
		let Self {
			analysis,
			sent,
			started,
			finished,
		} = self;
		let result = analysis
			.wait(|cues, status| {
				if cues.cues.len() > *sent || !*started || status.ended() {
					Some(status.error().map_or_else(
						|| {
							Ok(CueUpdate {
								cues: cues.cues.get(*sent..).unwrap_or_default().to_vec(),
								done: status.ended(),
							})
						},
						Err,
					))
				} else {
					None
				}
			})
			.await;
		*started = true;
		match &result {
			Ok(update) => {
				*sent += update.cues.len();
				*finished = update.done;
			}
			Err(_) => *finished = true,
		}
		Some(result)
	}
}

/// What an analysis task does with ffmpeg's output while it runs.
trait Absorber {
	/// A line ffmpeg wrote to stderr.
	fn line(&mut self, line: &str);
	/// Called every [`POLL`] while ffmpeg runs, and once after it exited.
	async fn tick(&mut self);
	/// Called after the last `tick` of a successful run.
	async fn finish(&mut self) {}
}

/// Runs `cmd` with the stderr lines and a poll clock going to `absorber`.
async fn drive(
	runner: &Runner,
	cmd: &FfmpegCommand,
	limits: &Limits,
	absorber: &mut impl Absorber,
) -> Result<(), RunError> {
	let (log_tx, mut log_rx) = mpsc::channel::<String>(LOG_CHANNEL);
	let run = runner.run_with_log(cmd, limits, None, Some(log_tx));
	tokio::pin!(run);
	let mut poll = tokio::time::interval(POLL);
	poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
	let mut log_open = true;
	let result = loop {
		tokio::select! {
			result = &mut run => break result,
			line = log_rx.recv(), if log_open => match line {
				Some(line) => absorber.line(&line),
				None => log_open = false,
			},
			_ = poll.tick() => absorber.tick().await,
		}
	};
	while let Some(line) = log_rx.recv().await {
		absorber.line(&line);
	}
	absorber.tick().await;
	result.map(|_| ())
}

/// The status an ffmpeg outcome ends an analysis with.
fn outcome(result: Result<(), RunError>, what: &str) -> Status {
	match result {
		Ok(()) => Status::Done,
		Err(RunError::Cancelled { .. }) => Status::Cancelled,
		Err(error) => {
			warn!(%error, "{what} analysis failed");
			Status::Failed(error.to_string())
		}
	}
}

struct ThumbsAbsorber {
	analysis: Arc<Analysis<Thumbs>>,
	dir: PathBuf,
	tiles: u32,
}

impl Absorber for ThumbsAbsorber {
	fn line(&mut self, line: &str) {
		if thumbs::is_tile_line(line) {
			self.tiles = self.tiles.saturating_add(1);
		}
	}

	/// Sprites are written atomically, so each file that exists is complete.
	async fn tick(&mut self) {
		let ready = self.analysis.lock().0.ready;
		let mut n = ready;
		while tokio::fs::try_exists(self.dir.join(format!("{n}.jpg")))
			.await
			.unwrap_or(false)
		{
			n += 1;
		}
		if n > ready {
			self.analysis.update(|thumbs| {
				thumbs.ready = n;
				true
			});
		}
	}

	async fn finish(&mut self) {
		let tiles = self.tiles;
		self.analysis.update(|thumbs| {
			thumbs.sheet = thumbs.sheet.clone().with_tiles(tiles);
			true
		});
	}
}

async fn thumbs_task(
	analysis: Arc<Analysis<Thumbs>>,
	runner: Runner,
	cmd: FfmpegCommand,
	dir: PathBuf,
	timeout: Duration,
) {
	if let Err(error) = tokio::fs::create_dir_all(&dir).await {
		analysis.finish(Status::Failed(format!(
			"cannot create {}: {error}",
			dir.display()
		)));
		return;
	}
	let limits = Limits {
		timeout: Some(timeout),
		cancel: analysis.cancel.clone(),
	};
	let mut absorber = ThumbsAbsorber {
		analysis: Arc::clone(&analysis),
		dir,
		tiles: 0,
	};
	let result = drive(&runner, &cmd, &limits, &mut absorber).await;
	let status = match outcome(result, "thumbnail") {
		Status::Done if absorber.tiles == 0 => {
			Status::Failed(String::from("ffmpeg produced no pictures"))
		}
		Status::Done => {
			absorber.finish().await;
			Status::Done
		}
		other => other,
	};
	analysis.finish(status);
}

struct PeaksAbsorber {
	analysis: Arc<Analysis<Peaks>>,
	parser: peaks::PeakParser,
	pending: Vec<Peak>,
}

impl Absorber for PeaksAbsorber {
	fn line(&mut self, line: &str) {
		if let Some(peak) = self.parser.push_line(line) {
			self.pending.push(peak);
		}
	}

	async fn tick(&mut self) {
		if self.pending.is_empty() {
			return;
		}
		let pending = std::mem::take(&mut self.pending);
		self.analysis.update(|peaks| {
			peaks.peaks.extend(pending);
			true
		});
	}
}

async fn peaks_task(
	analysis: Arc<Analysis<Peaks>>,
	runner: Runner,
	cmd: FfmpegCommand,
	timeout: Duration,
) {
	let limits = Limits {
		timeout: Some(timeout),
		cancel: analysis.cancel.clone(),
	};
	let mut absorber = PeaksAbsorber {
		analysis: Arc::clone(&analysis),
		parser: peaks::PeakParser::new(),
		pending: Vec::new(),
	};
	let result = drive(&runner, &cmd, &limits, &mut absorber).await;
	analysis.finish(outcome(result, "waveform"));
}

struct CuesAbsorber {
	analysis: Arc<Analysis<Cues>>,
	path: PathBuf,
}

impl CuesAbsorber {
	/// Re-reads the file ffmpeg writes and adds the cues that are new.
	async fn absorb(&self, complete: bool) {
		let Ok(bytes) = tokio::fs::read(&self.path).await else {
			return;
		};
		let parsed = cues::parse_webvtt(&String::from_utf8_lossy(&bytes), complete);
		self.analysis.update(|cues| {
			if parsed.len() > cues.cues.len() {
				cues.cues.extend_from_slice(&parsed[cues.cues.len()..]);
				true
			} else {
				false
			}
		});
	}
}

impl Absorber for CuesAbsorber {
	fn line(&mut self, _line: &str) {}

	async fn tick(&mut self) {
		self.absorb(false).await;
	}

	async fn finish(&mut self) {
		self.absorb(true).await;
	}
}

async fn cues_task(
	analysis: Arc<Analysis<Cues>>,
	runner: Runner,
	cmd: FfmpegCommand,
	output: PathBuf,
	timeout: Duration,
) {
	let limits = Limits {
		timeout: Some(timeout),
		cancel: analysis.cancel.clone(),
	};
	let mut absorber = CuesAbsorber {
		analysis: Arc::clone(&analysis),
		path: output,
	};
	let result = drive(&runner, &cmd, &limits, &mut absorber).await;
	let status = outcome(result, "subtitle");
	if status == Status::Done {
		absorber.finish().await;
	}
	analysis.finish(status);
}

/// A stderr line of ffmpeg without the `[Parsed_name_N @ 0x…] ` prefix a
/// filter's messages carry.
pub(crate) fn strip_filter_prefix(line: &str) -> &str {
	line.strip_prefix("[Parsed_")
		.and_then(|rest| rest.split_once("] "))
		.map_or(line, |(_, rest)| rest)
		.trim_start()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn filter_prefix_is_stripped() {
		assert_eq!(
			strip_filter_prefix("[Parsed_ametadata_3 @ 0x5581] frame:0 pts:0"),
			"frame:0 pts:0"
		);
		assert_eq!(strip_filter_prefix("frame:0 pts:0"), "frame:0 pts:0");
		assert_eq!(
			strip_filter_prefix("[aac @ 0x1] hello"),
			"[aac @ 0x1] hello"
		);
	}

	#[test]
	fn complete_bins_follow_the_analysed_time() {
		assert_eq!(complete_bins(0.0, 0.0, 1.0, 10), 0);
		assert_eq!(complete_bins(3.5, 0.0, 1.0, 10), 3);
		assert_eq!(complete_bins(3.5, 2.0, 0.5, 10), 3);
		assert_eq!(complete_bins(1.0, 2.0, 0.5, 10), 0);
		assert_eq!(complete_bins(100.0, 0.0, 1.0, 10), 10);
		assert_eq!(complete_bins(1.0, 0.0, 0.0, 10), 10);
	}

	#[test]
	fn image_subtitles_and_other_streams_are_refused() {
		let subtitle = |index: u32, codec: &str| Stream {
			index,
			codec_type: CodecType::Subtitle,
			codec_name: Some(codec.to_owned()),
			..Stream::default()
		};
		assert!(check_subtitle(&subtitle(2, "subrip")).is_ok());
		assert!(check_subtitle(&subtitle(2, "ass")).is_ok());
		assert!(matches!(
			check_subtitle(&subtitle(3, "hdmv_pgs_subtitle")),
			Err(TimelineError::ImageSubtitles { index: 3, codec }) if codec == "hdmv_pgs_subtitle"
		));
		assert!(matches!(
			check_subtitle(&subtitle(4, "dvd_subtitle")),
			Err(TimelineError::ImageSubtitles { index: 4, .. })
		));
		let audio = Stream {
			index: 1,
			codec_type: CodecType::Audio,
			..Stream::default()
		};
		assert!(matches!(
			check_subtitle(&audio),
			Err(TimelineError::NotSubtitle(1))
		));
	}

	#[test]
	fn window_bin_width() {
		let window = Window {
			from: 10.0,
			to: 20.0,
			count: 4,
		};
		assert!((window.bin_width() - 2.5).abs() < 1e-12);
	}

	#[tokio::test]
	async fn watches_report_a_finished_analysis_at_once() {
		let analysis = Analysis::new(Cues {
			cues: vec![Cue {
				start: 0.0,
				end: 1.0,
				text: String::from("hi"),
			}],
		});
		analysis.finish(Status::Done);
		let mut watch = CueWatch {
			analysis: Arc::clone(&analysis),
			sent: 0,
			started: false,
			finished: false,
		};
		let update = watch.next().await.unwrap().unwrap();
		assert_eq!(update.cues.len(), 1);
		assert!(update.done);
		assert!(watch.next().await.is_none());

		let analysis = Analysis::new(Peaks {
			bin_secs: 0.5,
			peaks: vec![
				Peak {
					min: -0.5,
					max: 0.5
				};
				4
			],
		});
		analysis.finish(Status::Failed(String::from("boom")));
		let mut watch = WaveformWatch {
			analysis,
			window: Window {
				from: 0.0,
				to: 2.0,
				count: 2,
			},
			sent: 0,
			started: false,
			finished: false,
		};
		assert_eq!(
			watch.next().await,
			Some(Err(AnalysisError::Failed(String::from("boom"))))
		);
		assert!(watch.next().await.is_none());
	}

	#[tokio::test]
	async fn a_waveform_watch_yields_peaks_as_they_are_analysed() {
		let analysis = Analysis::new(Peaks {
			bin_secs: 0.5,
			peaks: Vec::new(),
		});
		let mut watch = WaveformWatch {
			analysis: Arc::clone(&analysis),
			window: Window {
				from: 0.0,
				to: 4.0,
				count: 4,
			},
			sent: 0,
			started: false,
			finished: false,
		};
		// The first call answers at once, with nothing yet.
		let first = watch.next().await.unwrap().unwrap();
		assert_eq!((first.offset, first.peaks.len(), first.done), (0, 0, false));

		// Three base bins cover 1.5 s: one whole window bin.
		let pending = tokio::spawn(async move { (watch.next().await, watch) });
		analysis.update(|peaks| {
			peaks.peaks.extend(
				[Peak {
					min: -0.1,
					max: 0.1,
				}; 3],
			);
			true
		});
		let (second, mut watch) = pending.await.unwrap();
		let second = second.unwrap().unwrap();
		assert_eq!(second.offset, 0);
		assert_eq!(
			second.peaks,
			[Peak {
				min: -0.1,
				max: 0.1
			}]
		);
		assert!((second.analyzed_secs - 1.5).abs() < 1e-9);
		assert!(!second.done);

		// The end: the rest of the window, silence where data is missing.
		analysis.finish(Status::Done);
		let last = watch.next().await.unwrap().unwrap();
		assert_eq!(last.offset, 1);
		assert_eq!(
			last.peaks,
			[
				Peak {
					min: -0.1,
					max: 0.1
				},
				Peak::SILENCE,
				Peak::SILENCE
			]
		);
		assert!(last.done);
		assert!(watch.next().await.is_none());
	}
}
