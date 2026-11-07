//! The one place that starts ffmpeg and ffprobe.
//!
//! `clippy.toml` forbids `Command::new` everywhere else in the workspace, so
//! every process gets the same treatment: a timeout, cooperative
//! cancellation, progress parsing, a log of what ffmpeg said, and a
//! guarantee that no child outlives the call. When a run ends for any
//! reason, including the caller dropping the future, the process is killed
//! and reaped before the call returns.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::Path;
use std::pin::pin;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{self, AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::FfmpegCommand;
use super::command::ShellLine;
use super::probe::{self, ProbeError, ProbeResult};
use super::progress::{Progress, ProgressParser};
use super::tools::{Tool, Tools};

/// How many trailing stderr lines are kept for error reports.
const LOG_TAIL_LINES: usize = 64;

/// Spawns ffmpeg and ffprobe from the configured [`Tools`].
#[derive(Debug, Clone, Default)]
pub struct Runner {
	tools: Tools,
}

/// When a run must stop early.
#[derive(Debug, Clone, Default)]
pub struct Limits {
	/// Kill the process after this long. `None` means no limit.
	pub timeout: Option<Duration>,
	/// Kill the process when this token is cancelled.
	pub cancel: CancellationToken,
}

impl Limits {
	/// No timeout, cancellable through `cancel`.
	#[must_use]
	pub fn cancellable(cancel: CancellationToken) -> Self {
		Self {
			timeout: None,
			cancel,
		}
	}

	/// A timeout and no cancellation token.
	#[must_use]
	pub fn timeout(timeout: Duration) -> Self {
		Self {
			timeout: Some(timeout),
			cancel: CancellationToken::new(),
		}
	}
}

/// A process that exited successfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completed {
	/// The exit status, always successful.
	pub status: ExitStatus,
	/// The last lines ffmpeg wrote to stderr, for the job log.
	pub log_tail: String,
}

/// Why a run did not complete.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
	/// The program could not be started, usually because the path is wrong.
	#[error("could not start {tool} ({path}): {source}")]
	Spawn {
		/// Which program.
		tool: Tool,
		/// The path that was tried.
		path: String,
		/// The OS error.
		#[source]
		source: io::Error,
	},
	/// The program ran and reported failure.
	#[error("{tool} failed ({status}): {log_tail}")]
	Failed {
		/// Which program.
		tool: Tool,
		/// The exit status.
		status: ExitStatus,
		/// The last lines of stderr, which hold ffmpeg's error message.
		log_tail: String,
	},
	/// The cancellation token fired. The process is gone.
	#[error("{tool} was cancelled")]
	Cancelled {
		/// Which program.
		tool: Tool,
	},
	/// The timeout passed. The process is gone.
	#[error("{tool} did not finish within {timeout:?}")]
	TimedOut {
		/// Which program.
		tool: Tool,
		/// The limit that was exceeded.
		timeout: Duration,
	},
	/// Waiting for the process or reading its pipes failed.
	#[error("waiting for {tool}: {source}")]
	Wait {
		/// Which program.
		tool: Tool,
		/// The OS error.
		#[source]
		source: io::Error,
	},
	/// ffprobe succeeded but its output could not be read.
	#[error(transparent)]
	Probe(#[from] ProbeError),
}

impl Runner {
	/// A runner that starts the programs at `tools`.
	#[must_use]
	pub fn new(tools: Tools) -> Self {
		Self { tools }
	}

	/// The configured program paths.
	#[must_use]
	pub fn tools(&self) -> &Tools {
		&self.tools
	}

	/// The exact invocation [`Runner::run`] makes for `cmd`: the configured
	/// ffmpeg program followed by every argument, housekeeping flags
	/// included. This is what a job shows as its command.
	#[must_use]
	pub fn ffmpeg_invocation(&self, cmd: &FfmpegCommand) -> Vec<OsString> {
		let mut argv = vec![self.tools.ffmpeg.clone().into_os_string()];
		argv.extend(ffmpeg_argv(cmd));
		argv
	}

	/// Runs ffmpeg with `cmd` and waits for it to finish.
	///
	/// Each progress block ffmpeg reports is sent to `progress` as it
	/// arrives. A closed receiver is fine; the output is still drained.
	/// ffmpeg's stderr goes to the `ffmpeg` tracing target line by line and
	/// its tail is returned.
	///
	/// # Errors
	/// See [`RunError`]. When this returns, the process has exited.
	pub async fn run(
		&self,
		cmd: &FfmpegCommand,
		limits: &Limits,
		progress: Option<mpsc::Sender<Progress>>,
	) -> Result<Completed, RunError> {
		self.run_with_log(cmd, limits, progress, None).await
	}

	/// [`Runner::run`], additionally sending every stderr line to `log` as
	/// it arrives, for a live job log. A closed receiver stops the sending;
	/// the output is still drained and its tail returned.
	///
	/// # Errors
	/// See [`RunError`]. When this returns, the process has exited.
	pub async fn run_with_log(
		&self,
		cmd: &FfmpegCommand,
		limits: &Limits,
		progress: Option<mpsc::Sender<Progress>>,
		log: Option<mpsc::Sender<String>>,
	) -> Result<Completed, RunError> {
		let argv = ffmpeg_argv(cmd);
		tracing::info!(target: "ffmpeg", command = %ShellLine(&argv), "starting ffmpeg");
		let mut child = self.spawn(Tool::Ffmpeg, &argv)?;
		let stdout = child.stdout.take();
		let stderr = child.stderr.take();
		let pipes =
			async { tokio::join!(pump_progress(stdout, progress), collect_log(stderr, log)) };
		let span = tracing::info_span!("ffmpeg", pid = child.id());
		let (status, (), log_tail) = supervise(&mut child, Tool::Ffmpeg, limits, pipes)
			.instrument(span)
			.await?;
		if status.success() && !refused_to_overwrite(&log_tail) {
			Ok(Completed { status, log_tail })
		} else {
			Err(RunError::Failed {
				tool: Tool::Ffmpeg,
				status,
				log_tail,
			})
		}
	}

	/// Runs ffprobe on `path` and parses its report.
	///
	/// # Errors
	/// See [`RunError`]. ffprobe fails for files it cannot open or read.
	pub async fn probe(&self, path: &Path, limits: &Limits) -> Result<ProbeResult, RunError> {
		Ok(probe::parse(&self.probe_json(path, limits).await?)?)
	}

	/// Runs ffprobe on `path` and returns its report as the JSON text ffprobe
	/// wrote, for callers that need the fields [`ProbeResult`] leaves out.
	/// [`probe::parse`] turns it into a [`ProbeResult`].
	///
	/// # Errors
	/// See [`RunError`]. ffprobe fails for files it cannot open or read.
	pub async fn probe_json(&self, path: &Path, limits: &Limits) -> Result<String, RunError> {
		let argv = ffprobe_argv(path);
		tracing::info!(target: "ffprobe", command = %ShellLine(&argv), "starting ffprobe");
		let mut child = self.spawn(Tool::Ffprobe, &argv)?;
		let stdout = child.stdout.take();
		let stderr = child.stderr.take();
		let pipes = async { tokio::join!(read_all(stdout), collect_log(stderr, None)) };
		let span = tracing::info_span!("ffprobe", pid = child.id());
		let (status, json, log_tail) = supervise(&mut child, Tool::Ffprobe, limits, pipes)
			.instrument(span)
			.await?;
		if !status.success() {
			return Err(RunError::Failed {
				tool: Tool::Ffprobe,
				status,
				log_tail,
			});
		}
		Ok(json)
	}

	/// Measures how many bytes each stream of `path` takes, by summing the
	/// sizes of its packets, keyed by stream index. A stream without packets
	/// (an attachment, an empty subtitle track) is not in the result.
	///
	/// This demuxes the whole file (no decoding), so it costs one read of
	/// the file: fast from an SSD, long from a network share. The listing is
	/// summed as it arrives and never held in memory.
	///
	/// # Errors
	/// See [`RunError`].
	pub async fn packet_sizes(
		&self,
		path: &Path,
		limits: &Limits,
	) -> Result<BTreeMap<u32, u64>, RunError> {
		let argv = ffprobe_packet_sizes_argv(path);
		tracing::info!(target: "ffprobe", command = %ShellLine(&argv), "starting ffprobe");
		let mut child = self.spawn(Tool::Ffprobe, &argv)?;
		let stdout = child.stdout.take();
		let stderr = child.stderr.take();
		let pipes = async { tokio::join!(sum_packets(stdout), collect_log(stderr, None)) };
		let span = tracing::info_span!("ffprobe", pid = child.id());
		let (status, sizes, log_tail) = supervise(&mut child, Tool::Ffprobe, limits, pipes)
			.instrument(span)
			.await?;
		if !status.success() {
			return Err(RunError::Failed {
				tool: Tool::Ffprobe,
				status,
				log_tail,
			});
		}
		Ok(sizes.into_bytes())
	}

	/// Runs `tool` with plain `args` (no runner flags) and returns its stdout.
	///
	/// For short queries such as `ffmpeg -encoders`. Nothing in the output is
	/// interpreted here.
	///
	/// # Errors
	/// See [`RunError`]. A non-zero exit is [`RunError::Failed`].
	pub async fn capture(
		&self,
		tool: Tool,
		args: &[&str],
		limits: &Limits,
	) -> Result<String, RunError> {
		let command: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
		tracing::debug!(target: "ffmpeg", command = %ShellLine(&command), "starting {tool} query");
		let mut child = self.spawn(tool, &command)?;
		let stdout = child.stdout.take();
		let stderr = child.stderr.take();
		let pipes = async { tokio::join!(read_all(stdout), collect_log(stderr, None)) };
		let (status, text, log_tail) = supervise(&mut child, tool, limits, pipes).await?;
		if status.success() {
			Ok(text)
		} else {
			Err(RunError::Failed {
				tool,
				status,
				log_tail,
			})
		}
	}

	/// Lists the keyframe times of stream `stream_index` of `path`, ascending.
	///
	/// This demuxes the whole file (no decoding), so it costs one read of
	/// the file: fast from an SSD, long from a network share.
	///
	/// # Errors
	/// See [`RunError`].
	pub async fn keyframes(
		&self,
		path: &Path,
		stream_index: u32,
		limits: &Limits,
	) -> Result<Vec<f64>, RunError> {
		let argv = ffprobe_keyframes_argv(path, stream_index);
		tracing::info!(target: "ffprobe", command = %ShellLine(&argv), "starting ffprobe");
		let mut child = self.spawn(Tool::Ffprobe, &argv)?;
		let stdout = child.stdout.take();
		let stderr = child.stderr.take();
		let pipes = async { tokio::join!(read_all(stdout), collect_log(stderr, None)) };
		let span = tracing::info_span!("ffprobe", pid = child.id());
		let (status, csv, log_tail) = supervise(&mut child, Tool::Ffprobe, limits, pipes)
			.instrument(span)
			.await?;
		if !status.success() {
			return Err(RunError::Failed {
				tool: Tool::Ffprobe,
				status,
				log_tail,
			});
		}
		Ok(probe::parse_keyframes(&csv))
	}

	/// Lists the keyframes of stream `stream_index` of `path` within
	/// `intervals`, each a `(from, to)` in seconds of the container's own
	/// timestamps, ascending by presentation time.
	///
	/// ffprobe seeks to each interval's start and reads to its end, so
	/// this costs a read of the intervals only, not of the file. The seek
	/// lands at or before the start (on the previous keyframe for a
	/// container with a keyframe index, somewhere in the previous GOP for
	/// one without), so the result can begin a little before an interval.
	///
	/// # Errors
	/// See [`RunError`].
	pub async fn keyframes_in(
		&self,
		path: &Path,
		stream_index: u32,
		intervals: &[(f64, f64)],
		limits: &Limits,
	) -> Result<Vec<probe::Keyframe>, RunError> {
		let argv = ffprobe_keyframes_in_argv(path, stream_index, intervals);
		tracing::info!(target: "ffprobe", command = %ShellLine(&argv), "starting ffprobe");
		let mut child = self.spawn(Tool::Ffprobe, &argv)?;
		let stdout = child.stdout.take();
		let stderr = child.stderr.take();
		let pipes = async { tokio::join!(read_all(stdout), collect_log(stderr, None)) };
		let span = tracing::info_span!("ffprobe", pid = child.id());
		let (status, csv, log_tail) = supervise(&mut child, Tool::Ffprobe, limits, pipes)
			.instrument(span)
			.await?;
		if !status.success() {
			return Err(RunError::Failed {
				tool: Tool::Ffprobe,
				status,
				log_tail,
			});
		}
		Ok(probe::parse_keyframe_packets(&csv))
	}

	/// Lists every packet of stream `stream_index` of `path` within
	/// `intervals`, as [`Runner::keyframes_in`] lists the keyframes: in the
	/// order read, which is decode order, with the same seeking and the
	/// same cost. Where the intervals overlap, packets are listed twice.
	///
	/// # Errors
	/// See [`RunError`].
	pub async fn packets_in(
		&self,
		path: &Path,
		stream_index: u32,
		intervals: &[(f64, f64)],
		limits: &Limits,
	) -> Result<Vec<probe::Packet>, RunError> {
		let argv = ffprobe_keyframes_in_argv(path, stream_index, intervals);
		tracing::info!(target: "ffprobe", command = %ShellLine(&argv), "starting ffprobe");
		let mut child = self.spawn(Tool::Ffprobe, &argv)?;
		let stdout = child.stdout.take();
		let stderr = child.stderr.take();
		let pipes = async { tokio::join!(read_all(stdout), collect_log(stderr, None)) };
		let span = tracing::info_span!("ffprobe", pid = child.id());
		let (status, csv, log_tail) = supervise(&mut child, Tool::Ffprobe, limits, pipes)
			.instrument(span)
			.await?;
		if !status.success() {
			return Err(RunError::Failed {
				tool: Tool::Ffprobe,
				status,
				log_tail,
			});
		}
		Ok(probe::parse_packets(&csv))
	}

	/// Starts `tool` with `argv`, piping stdout and stderr and closing stdin.
	// Allowed here and nowhere else; see `clippy.toml`.
	#[expect(
		clippy::disallowed_methods,
		reason = "the runner is the one process spawner"
	)]
	fn spawn(&self, tool: Tool, argv: &[OsString]) -> Result<Child, RunError> {
		let path = self.tools.path(tool);
		Command::new(path)
			.args(argv)
			.stdin(Stdio::null())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			// Safety net for a dropped future; `supervise` kills explicitly.
			.kill_on_drop(true)
			.spawn()
			.map_err(|source| RunError::Spawn {
				tool,
				path: path.display().to_string(),
				source,
			})
	}
}

/// The full ffmpeg argument list: the runner's housekeeping flags, then the
/// operation's arguments.
///
/// - `-nostdin`: never wait for a key press.
/// - `-nostats`: no carriage-return progress line on stderr; progress comes
///   as `-progress` blocks on stdout instead.
/// - `-progress pipe:1`: machine-readable progress on stdout.
pub(crate) fn ffmpeg_argv(cmd: &FfmpegCommand) -> Vec<OsString> {
	let mut argv: Vec<OsString> = [
		"-hide_banner",
		"-nostdin",
		"-nostats",
		"-progress",
		"pipe:1",
	]
	.iter()
	.map(Into::into)
	.collect();
	argv.extend_from_slice(cmd.as_args());
	argv
}

/// The ffprobe argument list for a full report on `path`.
pub(crate) fn ffprobe_argv(path: &Path) -> Vec<OsString> {
	let mut argv: Vec<OsString> = [
		"-hide_banner",
		"-v",
		"error",
		"-print_format",
		"json",
		"-show_format",
		"-show_streams",
		"-show_chapters",
		"-i",
	]
	.iter()
	.map(Into::into)
	.collect();
	argv.push(path.into());
	argv
}

/// The ffprobe argument list for the packets of one stream of `path`, as
/// CSV lines of `pts_time,dts_time,flags`; see [`probe::parse_keyframes`].
pub(crate) fn ffprobe_keyframes_argv(path: &Path, stream_index: u32) -> Vec<std::ffi::OsString> {
	let mut argv: Vec<std::ffi::OsString> = [
		"-hide_banner",
		"-v",
		"error",
		"-select_streams",
		&stream_index.to_string(),
		"-show_entries",
		"packet=pts_time,dts_time,flags",
		"-of",
		"csv=p=0",
		"-i",
	]
	.iter()
	.map(Into::into)
	.collect();
	argv.push(path.into());
	argv
}

/// The ffprobe argument list for the packets of one stream of `path`
/// within `intervals`, as [`ffprobe_keyframes_argv`] lists them. Each
/// interval is `from%to` in seconds; ffprobe seeks to `from` and stops
/// after `to`.
pub(crate) fn ffprobe_keyframes_in_argv(
	path: &Path,
	stream_index: u32,
	intervals: &[(f64, f64)],
) -> Vec<OsString> {
	let intervals: Vec<String> = intervals
		.iter()
		.map(|(from, to)| format!("{from:.6}%{to:.6}"))
		.collect();
	let mut argv: Vec<OsString> = [
		"-hide_banner",
		"-v",
		"error",
		"-select_streams",
		&stream_index.to_string(),
		"-read_intervals",
		&intervals.join(","),
		"-show_entries",
		"packet=pts_time,dts_time,flags",
		"-of",
		"csv=p=0",
		"-i",
	]
	.iter()
	.map(Into::into)
	.collect();
	argv.push(path.into());
	argv
}

/// The ffprobe argument list for the packets of every stream of `path`, as
/// CSV lines of `stream_index,size`; see [`probe::PacketSizes`].
pub(crate) fn ffprobe_packet_sizes_argv(path: &Path) -> Vec<OsString> {
	let mut argv: Vec<OsString> = [
		"-hide_banner",
		"-v",
		"error",
		"-show_entries",
		"packet=stream_index,size",
		"-of",
		"csv=p=0",
		"-i",
	]
	.iter()
	.map(Into::into)
	.collect();
	argv.push(path.into());
	argv
}

/// Waits for `child` while `pipes` drains its output, enforcing `limits`.
///
/// `pipes` yields what it read from stdout and the stderr log tail. Returns
/// the exit status together with both. On cancellation or timeout the child
/// is killed and reaped, `pipes` is run to completion (the pipes close when
/// the process dies) and the log tail is written to the log instead.
async fn supervise<T>(
	child: &mut Child,
	tool: Tool,
	limits: &Limits,
	pipes: impl Future<Output = (T, String)>,
) -> Result<(ExitStatus, T, String), RunError> {
	let mut pipes = pin!(pipes);
	let deadline = async {
		match limits.timeout {
			Some(timeout) => time::sleep(timeout).await,
			None => std::future::pending().await,
		}
	};
	let outcome = tokio::select! {
		biased;
		() = limits.cancel.cancelled() => Err(RunError::Cancelled { tool }),
		() = deadline => Err(RunError::TimedOut { tool, timeout: limits.timeout.unwrap_or_default() }),
		result = async {
			let (output, log_tail) = pipes.as_mut().await;
			child.wait().await.map(|status| (status, output, log_tail))
		} => result.map_err(|source| RunError::Wait { tool, source }),
	};
	match outcome {
		Ok((status, output, log_tail)) => {
			tracing::info!(target: "ffmpeg", %status, "{tool} exited");
			Ok((status, output, log_tail))
		}
		Err(error) => {
			// `start_kill` errors only when the process already exited; `wait`
			// then reaps it either way.
			if let Err(e) = child.start_kill() {
				tracing::debug!(target: "ffmpeg", error = %e, "{tool} kill");
			}
			let status = child
				.wait()
				.await
				.map_err(|source| RunError::Wait { tool, source })?;
			let (_, log_tail) = pipes.await;
			tracing::warn!(target: "ffmpeg", %status, %error, "{tool} stopped early\n{log_tail}");
			Err(error)
		}
	}
}

/// Parses `-progress` blocks from stdout and forwards each one.
async fn pump_progress(
	stdout: Option<impl AsyncRead + Unpin>,
	mut progress: Option<mpsc::Sender<Progress>>,
) {
	let Some(stdout) = stdout else { return };
	let mut lines = BufReader::new(stdout).lines();
	let mut parser = ProgressParser::new();
	loop {
		let line = match lines.next_line().await {
			Ok(Some(line)) => line,
			Ok(None) => break,
			Err(e) => {
				tracing::debug!(target: "ffmpeg", error = %e, "progress pipe closed");
				break;
			}
		};
		let Some(report) = parser.push_line(&line) else {
			continue;
		};
		tracing::trace!(target: "ffmpeg", ?report, "progress");
		if let Some(tx) = &progress
			&& tx.send(report).await.is_err()
		{
			// Nobody listens any more; keep draining so ffmpeg never blocks.
			progress = None;
		}
	}
}

/// Logs every stderr line, forwards it to `log` and returns the last
/// [`LOG_TAIL_LINES`] of them.
async fn collect_log(
	stderr: Option<impl AsyncRead + Unpin>,
	mut log: Option<mpsc::Sender<String>>,
) -> String {
	let Some(stderr) = stderr else {
		return String::new();
	};
	let mut lines = BufReader::new(stderr).lines();
	let mut tail = std::collections::VecDeque::with_capacity(LOG_TAIL_LINES);
	while let Ok(Some(line)) = lines.next_line().await {
		tracing::debug!(target: "ffmpeg", "{line}");
		if let Some(tx) = &log
			&& tx.send(line.clone()).await.is_err()
		{
			// Nobody listens any more; keep draining so ffmpeg never blocks.
			log = None;
		}
		if tail.len() == LOG_TAIL_LINES {
			tail.pop_front();
		}
		tail.push_back(line);
	}
	tail.into_iter().collect::<Vec<_>>().join("\n")
}

/// Sums a packet listing line by line; see [`probe::PacketSizes`].
async fn sum_packets(stdout: Option<impl AsyncRead + Unpin>) -> probe::PacketSizes {
	let mut sizes = probe::PacketSizes::default();
	let Some(stdout) = stdout else {
		return sizes;
	};
	let mut lines = BufReader::new(stdout).lines();
	loop {
		match lines.next_line().await {
			Ok(Some(line)) => sizes.push_line(&line),
			Ok(None) => break,
			Err(e) => {
				tracing::debug!(target: "ffprobe", error = %e, "packet pipe closed");
				break;
			}
		}
	}
	sizes
}

/// Reads a pipe to the end as text.
async fn read_all(pipe: Option<impl AsyncRead + Unpin>) -> String {
	let Some(mut pipe) = pipe else {
		return String::new();
	};
	let mut text = String::new();
	if let Err(e) = pipe.read_to_string(&mut text).await {
		tracing::debug!(target: "ffprobe", error = %e, "output pipe closed");
	}
	text
}

/// Whether ffmpeg stopped because an output file exists. ffmpeg 8.1 and
/// older exit with 0 then, although nothing was written (newer builds exit
/// with an error), so the log decides.
fn refused_to_overwrite(log_tail: &str) -> bool {
	log_tail.lines().any(|line| {
		line.contains("already exists. Exiting") || line.contains("Not overwriting - exiting")
	})
}

#[cfg(test)]
mod tests {
	use super::super::operations::remux;
	use super::*;

	#[test]
	fn a_refused_overwrite_is_noticed_in_the_log() {
		let tail =
			"Error opening output file /m/out.mkv.\nFile '/m/out.mkv' already exists. Exiting.";
		assert!(refused_to_overwrite(tail));
		assert!(refused_to_overwrite("Not overwriting - exiting"));
		assert!(!refused_to_overwrite("video:12kB audio:3kB subtitle:0kB"));
	}

	#[test]
	fn ffmpeg_argv_adds_runner_flags_before_the_operation() {
		let argv = ffmpeg_argv(&remux(Path::new("in.mkv"), Path::new("out dir/out.mp4")));
		insta::assert_snapshot!(ShellLine(&argv));
	}

	#[test]
	fn ffprobe_argv_asks_for_a_full_json_report() {
		insta::assert_snapshot!(ShellLine(&ffprobe_argv(Path::new("/media/in/movie.mkv"))));
	}

	#[test]
	fn ffprobe_packet_sizes_argv_lists_every_packet_as_csv() {
		insta::assert_snapshot!(ShellLine(&ffprobe_packet_sizes_argv(Path::new(
			"/media/in/movie.mkv"
		))));
	}

	#[test]
	fn ffprobe_keyframes_argv_lists_one_stream_as_csv() {
		insta::assert_snapshot!(ShellLine(&ffprobe_keyframes_argv(
			Path::new("/media/in/movie.mkv"),
			3
		)));
	}

	#[test]
	fn ffprobe_keyframes_in_argv_reads_only_the_intervals() {
		insta::assert_snapshot!(ShellLine(&ffprobe_keyframes_in_argv(
			Path::new("/media/in/movie.mkv"),
			0,
			&[(0.0, 30.0), (3570.5, 3630.5)]
		)));
	}

	#[tokio::test]
	async fn missing_program_is_a_spawn_error() {
		let runner = Runner::new(Tools::in_dir(Path::new("/nonexistent/mmt")));
		let err = runner
			.probe(Path::new("x.mkv"), &Limits::default())
			.await
			.unwrap_err();
		assert!(
			matches!(
				err,
				RunError::Spawn {
					tool: Tool::Ffprobe,
					..
				}
			),
			"{err}"
		);
		let err = runner
			.run(&FfmpegCommand::new(), &Limits::default(), None)
			.await
			.unwrap_err();
		assert!(
			matches!(
				err,
				RunError::Spawn {
					tool: Tool::Ffmpeg,
					..
				}
			),
			"{err}"
		);
	}
}
