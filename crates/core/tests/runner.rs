//! Runs the real ffmpeg and ffprobe from `PATH`. Test clips are generated
//! here with ffmpeg's test sources, so no media is committed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{
	FfmpegCommand, Limits, Progress, RunError, Runner, Tool, operations,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Two seconds of test pattern and tone, H.264 + AAC in MP4.
async fn make_clip(runner: &Runner, dir: &Path) -> Result<PathBuf, RunError> {
	let clip = dir.join("clip.mp4");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=duration=2:size=160x120:rate=25")
		.option("-f", "lavfi")
		.input("sine=frequency=440:duration=2")
		.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-c:a", "aac")
		.output(&clip);
	runner
		.run(&cmd, &Limits::timeout(Duration::from_secs(60)), None)
		.await?;
	Ok(clip)
}

/// An ffmpeg command that would run for ten minutes at real-time pace.
/// `marker` ends up in its argument list, so the process can be found.
fn endless(marker: &str) -> FfmpegCommand {
	FfmpegCommand::new()
		.flag("-re")
		.option("-f", "lavfi")
		.input("testsrc2=duration=600:size=160x120:rate=25")
		.option("-metadata", format!("comment={marker}"))
		.option("-f", "null")
		.output("-")
}

/// Whether any process on this machine has an argument ending in `marker`.
/// Linux only: it reads `/proc`, which is the one way to look for a process
/// without spawning one.
fn process_with_argument_exists(marker: &str) -> bool {
	if !cfg!(target_os = "linux") {
		return false;
	}
	std::fs::read_dir("/proc")
		.into_iter()
		.flatten()
		.flatten()
		.filter_map(|entry| std::fs::read(entry.path().join("cmdline")).ok())
		.any(|cmdline| {
			cmdline
				.split(|b| *b == 0)
				.any(|arg| arg.ends_with(marker.as_bytes()))
		})
}

#[tokio::test]
async fn remux_reports_progress_and_completes() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(&runner, dir.path()).await.unwrap();

	let (tx, mut rx) = mpsc::channel(16);
	let output = dir.path().join("remuxed.mkv");
	let cmd = operations::remux(&clip, &output);
	let completed = runner
		.run(&cmd, &Limits::timeout(Duration::from_secs(60)), Some(tx))
		.await
		.unwrap();
	assert!(completed.status.success());
	assert!(output.is_file());

	let mut reports: Vec<Progress> = Vec::new();
	while let Some(report) = rx.recv().await {
		reports.push(report);
	}
	let last = reports.last().expect("at least the final progress block");
	assert!(last.end, "{last:?}");
	assert!(
		last.out_time
			.is_some_and(|t| t >= Duration::from_millis(1900)),
		"{last:?}"
	);
	assert!(last.total_size.is_some_and(|s| s > 0), "{last:?}");
	assert!(last.speed.is_some(), "{last:?}");
	assert!(last.fps.is_some(), "{last:?}");
	assert!(reports[..reports.len() - 1].iter().all(|r| !r.end));
}

#[tokio::test]
async fn failed_run_reports_ffmpeg_log() {
	let dir = tempfile::tempdir().unwrap();
	let cmd = operations::remux(&dir.path().join("missing.mkv"), &dir.path().join("out.mp4"));
	let err = Runner::default()
		.run(&cmd, &Limits::default(), None)
		.await
		.unwrap_err();
	match err {
		RunError::Failed {
			tool: Tool::Ffmpeg,
			status,
			log_tail,
		} => {
			assert!(!status.success());
			assert!(log_tail.contains("missing.mkv"), "{log_tail}");
		}
		other => panic!("unexpected {other}"),
	}
}

#[tokio::test]
async fn cancel_kills_ffmpeg_without_orphans() {
	let marker = format!("mmt-cancel-{}", std::process::id());
	let (tx, mut rx) = mpsc::channel(16);
	let cancel = CancellationToken::new();
	let limits = Limits::cancellable(cancel.clone());
	let cmd = endless(&marker);
	let job = tokio::spawn(async move { Runner::default().run(&cmd, &limits, Some(tx)).await });

	// ffmpeg is up and producing output once the first progress block arrives.
	let first = tokio::time::timeout(Duration::from_secs(30), rx.recv())
		.await
		.expect("progress arrives");
	assert!(first.is_some_and(|p| !p.end));
	if cfg!(target_os = "linux") {
		assert!(process_with_argument_exists(&marker), "ffmpeg is running");
	}

	cancel.cancel();
	let result = tokio::time::timeout(Duration::from_secs(10), job)
		.await
		.expect("run returns")
		.unwrap();
	assert!(
		matches!(result, Err(RunError::Cancelled { tool: Tool::Ffmpeg })),
		"{result:?}"
	);
	assert!(!process_with_argument_exists(&marker), "ffmpeg was reaped");
}

#[tokio::test]
async fn timeout_kills_ffmpeg_without_orphans() {
	let marker = format!("mmt-timeout-{}", std::process::id());
	let started = std::time::Instant::now();
	let limits = Limits::timeout(Duration::from_millis(1500));
	let result = Runner::default()
		.run(&endless(&marker), &limits, None)
		.await;
	assert!(
		matches!(result, Err(RunError::TimedOut { tool: Tool::Ffmpeg, timeout }) if timeout == Duration::from_millis(1500)),
		"{result:?}"
	);
	assert!(
		started.elapsed() < Duration::from_secs(30),
		"did not wait for the full clip"
	);
	assert!(!process_with_argument_exists(&marker), "ffmpeg was reaped");
}

#[tokio::test]
async fn probe_returns_typed_streams() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(&runner, dir.path()).await.unwrap();

	let probe = runner
		.probe(&clip, &Limits::timeout(Duration::from_secs(30)))
		.await
		.unwrap();
	assert!(
		probe.format.format_name.contains("mp4"),
		"{}",
		probe.format.format_name
	);
	assert!(probe.format.size.is_some_and(|s| s > 0));
	assert!(
		probe
			.duration_secs()
			.is_some_and(|d| (1.9..=2.2).contains(&d)),
		"{:?}",
		probe.duration_secs()
	);

	let video = probe
		.streams_of(CodecType::Video)
		.next()
		.expect("video stream");
	assert_eq!(video.codec_name.as_deref(), Some("h264"));
	assert_eq!((video.width, video.height), (Some(160), Some(120)));
	assert_eq!(video.frame_rate(), Some(25.0));
	assert_eq!(video.nb_frames, Some(50));

	let audio = probe
		.streams_of(CodecType::Audio)
		.next()
		.expect("audio stream");
	assert_eq!(audio.codec_name.as_deref(), Some("aac"));
	assert_eq!(audio.sample_rate, Some(44_100));
	assert_eq!(audio.channels, Some(1));
}

#[tokio::test]
async fn probe_of_missing_file_fails_with_ffprobe_message() {
	let err = Runner::default()
		.probe(Path::new("/nonexistent/clip.mp4"), &Limits::default())
		.await
		.unwrap_err();
	match err {
		RunError::Failed {
			tool: Tool::Ffprobe,
			log_tail,
			..
		} => {
			assert!(log_tail.contains("clip.mp4"), "{log_tail}");
		}
		other => panic!("unexpected {other}"),
	}
}
