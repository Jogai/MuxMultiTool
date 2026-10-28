//! The job queue: every operation that writes output runs as a job.
//!
//! A job is an [`FfmpegCommand`] with a name. The [`Queue`] starts jobs in
//! submission order, at most `concurrency` at a time, and keeps each one's
//! state, latest progress, log tail and error. Jobs live in the server
//! process, so they keep running when the browser tab closes, and they are
//! gone when the server restarts: the queue is in memory only.
//!
//! ```text
//! submit ──▶ Queued ──▶ Running ──▶ Succeeded | Failed
//!               │           │
//!               └─ cancel ──┴─────▶ Cancelled
//! ```
//!
//! Clients follow a job or the whole queue through a [`Watch`]: a snapshot
//! first, then every change as an [`Event`]. Event ids increase across all
//! jobs, and each job keeps a short buffer of its events, so a client that
//! lost its stream resumes from the last id it saw (see [`events`]).

use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::ffmpeg::{FfmpegCommand, Progress};

pub mod events;
pub mod explain;
pub mod queue;

pub use events::{Event, EventKind, Snapshot, WatchItem};
pub use explain::explain;
pub use queue::{Queue, QueueConfig, QueueError, Watch};

/// How many trailing log lines a [`Job`] keeps. The full stream of lines
/// goes out as [`EventKind::Log`] events.
pub const JOB_LOG_LINES: usize = 200;

/// The id of a job: an opaque string, unique for the lifetime of the server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(String);

impl JobId {
	/// The id as text, for the API and the log.
	#[must_use]
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl From<String> for JobId {
	fn from(id: String) -> Self {
		Self(id)
	}
}

impl From<&str> for JobId {
	fn from(id: &str) -> Self {
		Self(id.to_owned())
	}
}

impl fmt::Display for JobId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}

/// Where a job is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobState {
	/// Waiting for a free slot.
	Queued,
	/// ffmpeg is running.
	Running,
	/// ffmpeg exited successfully.
	Succeeded,
	/// ffmpeg failed or could not be started; see [`Job::error`].
	Failed,
	/// Cancelled by a client, or by the server shutting down.
	Cancelled,
}

impl JobState {
	/// `true` once a job can no longer change: it succeeded, failed or was
	/// cancelled.
	#[must_use]
	pub const fn is_terminal(self) -> bool {
		matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
	}
}

impl fmt::Display for JobState {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::Queued => "queued",
			Self::Running => "running",
			Self::Succeeded => "succeeded",
			Self::Failed => "failed",
			Self::Cancelled => "cancelled",
		})
	}
}

/// What to run, as given to [`Queue::submit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
	/// The label shown in the queue, e.g. the output file name.
	pub name: String,
	/// The operation's ffmpeg arguments.
	pub command: FfmpegCommand,
	/// The expected duration of the output, when the operation knows it.
	/// It turns ffmpeg's position reports into a percentage and an ETA.
	pub expected_duration: Option<Duration>,
	/// Commands to run before `command`, in order, each to completion: for
	/// an operation that needs more than one ffmpeg run, such as a smart
	/// cut, which writes its pieces first and joins them after. The job
	/// fails at the first command that fails. The percentage counts every
	/// command as an equal share, each measured against
	/// `expected_duration`; the shown command line is all of them joined
	/// with `&&`.
	pub preparation: Vec<FfmpegCommand>,
	/// A folder of the job's own for intermediate files, which must exist
	/// when the job starts. It is removed with everything in it when the
	/// job ends, however it ends.
	pub scratch_dir: Option<PathBuf>,
}

/// Why a job failed, told twice: in plain language for the user and as
/// ffmpeg's own output for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobError {
	/// The plain-language explanation, e.g. "the input file was not found".
	pub message: String,
	/// The tail of what ffmpeg wrote to stderr; empty when it never started.
	pub ffmpeg_output: String,
}

/// One of ffmpeg's progress reports, plus what the queue derives from it
/// when the job's expected duration is known.
#[derive(Debug, Clone, PartialEq)]
pub struct JobProgress {
	/// The report as ffmpeg sent it.
	pub report: Progress,
	/// Share of the expected duration that is done, 0 to 100.
	pub percent: Option<f64>,
	/// Estimated time left, from the expected duration and the speed.
	pub eta: Option<Duration>,
}

impl JobProgress {
	/// Derives the percentage and ETA of `report` for a job that expects
	/// to produce `expected` of media. Both stay `None` without it.
	#[must_use]
	pub fn new(report: Progress, expected: Option<Duration>) -> Self {
		let expected = expected.filter(|d| !d.is_zero());
		let (percent, eta) = if report.end {
			(expected.map(|_| 100.0), expected.map(|_| Duration::ZERO))
		} else {
			let position = expected.zip(report.out_time);
			let percent = position.map(|(total, done)| {
				(done.as_secs_f64() / total.as_secs_f64() * 100.0).clamp(0.0, 100.0)
			});
			let eta = position
				.zip(report.speed)
				.and_then(|((total, done), speed)| {
					if !speed.is_finite() || speed <= 0.0 {
						return None;
					}
					Duration::try_from_secs_f64(total.saturating_sub(done).as_secs_f64() / speed)
						.ok()
				});
			(percent, eta)
		};
		Self {
			report,
			percent,
			eta,
		}
	}
}

/// A job as the queue sees it: a snapshot that a [`Watch`] keeps current.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
	/// The job's id.
	pub id: JobId,
	/// See [`JobSpec::name`].
	pub name: String,
	/// Where the job is in its life.
	pub state: JobState,
	/// The exact command the job runs: the ffmpeg program followed by every
	/// argument, as the runner starts it. Known from submission on.
	pub argv: Vec<String>,
	/// [`Job::argv`] as one line, quoted for a shell.
	pub command_line: String,
	/// The latest progress report, `None` until ffmpeg sent the first one.
	pub progress: Option<JobProgress>,
	/// The last [`JOB_LOG_LINES`] lines ffmpeg wrote to stderr.
	pub log: Vec<String>,
	/// Why the job failed; set when the state is [`JobState::Failed`].
	pub error: Option<JobError>,
	/// When the job was submitted.
	pub created: SystemTime,
	/// When ffmpeg was started.
	pub started: Option<SystemTime>,
	/// When the job reached a terminal state.
	pub finished: Option<SystemTime>,
}

#[cfg(test)]
mod tests {
	use super::*;

	fn report(secs: u64, speed: Option<f64>, end: bool) -> Progress {
		Progress {
			out_time: Some(Duration::from_secs(secs)),
			speed,
			end,
			..Progress::default()
		}
	}

	#[test]
	fn percent_and_eta_need_the_expected_duration() {
		let p = JobProgress::new(report(30, Some(2.0), false), None);
		assert_eq!((p.percent, p.eta), (None, None));

		let p = JobProgress::new(report(30, Some(2.0), false), Some(Duration::from_secs(120)));
		assert_eq!(p.percent, Some(25.0));
		assert_eq!(p.eta, Some(Duration::from_secs(45)));
	}

	#[test]
	fn eta_needs_a_positive_speed() {
		let expected = Some(Duration::from_secs(120));
		assert_eq!(
			JobProgress::new(report(30, None, false), expected).eta,
			None
		);
		assert_eq!(
			JobProgress::new(report(30, Some(0.0), false), expected).eta,
			None
		);
		assert_eq!(
			JobProgress::new(report(30, Some(f64::INFINITY), false), expected).eta,
			None
		);
	}

	#[test]
	fn the_end_report_is_complete_and_overrun_is_clamped() {
		let expected = Some(Duration::from_secs(10));
		let done = JobProgress::new(report(3, Some(1.0), true), expected);
		assert_eq!(
			(done.percent, done.eta),
			(Some(100.0), Some(Duration::ZERO))
		);

		let over = JobProgress::new(report(12, Some(1.0), false), expected);
		assert_eq!(
			(over.percent, over.eta),
			(Some(100.0), Some(Duration::ZERO))
		);

		let zero = JobProgress::new(report(1, Some(1.0), false), Some(Duration::ZERO));
		assert_eq!((zero.percent, zero.eta), (None, None));
	}

	#[test]
	fn terminal_states() {
		assert!(!JobState::Queued.is_terminal());
		assert!(!JobState::Running.is_terminal());
		assert!(JobState::Succeeded.is_terminal());
		assert!(JobState::Failed.is_terminal());
		assert!(JobState::Cancelled.is_terminal());
		assert_eq!(JobState::Cancelled.to_string(), "cancelled");
	}
}
