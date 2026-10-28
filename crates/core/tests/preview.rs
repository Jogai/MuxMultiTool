//! Preview sessions against the real ffmpeg and ffprobe from `PATH`. Test
//! clips are generated with ffmpeg's test sources, so no media is committed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, RunError, Runner};
use muxmultitool_core::preview::{Mode, PreviewConfig, PreviewManager, SegmentError};

const LIMITS: Duration = Duration::from_secs(120);

/// Writes `name` into `dir`: `secs` of test pattern and tone, encoded with
/// `video` and `audio` options. `keyframes` forces keyframes at those times
/// (H.264 only); without it the encoder decides.
async fn make_clip(
	runner: &Runner,
	dir: &Path,
	name: &str,
	secs: u32,
	video: &[&str],
	audio: &[&str],
	keyframes: Option<&str>,
) -> Result<PathBuf, RunError> {
	let clip = dir.join(name);
	let mut cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input(format!("testsrc2=size=160x120:rate=25:duration={secs}"))
		.option("-f", "lavfi")
		.input(format!(
			"sine=frequency=440:sample_rate=48000:duration={secs}"
		))
		.args(video)
		.args(audio);
	if let Some(times) = keyframes {
		cmd = cmd
			.option("-g", "9999")
			.option("-sc_threshold", "0")
			.option("-force_key_frames", times);
	}
	runner
		.run(&cmd.output(&clip), &Limits::timeout(LIMITS), None)
		.await?;
	Ok(clip)
}

const H264: &[&str] = &[
	"-c:v",
	"libx264",
	"-preset",
	"ultrafast",
	"-pix_fmt",
	"yuv420p",
];
const AAC: &[&str] = &["-c:a", "aac", "-b:a", "64k"];

fn config() -> PreviewConfig {
	PreviewConfig {
		run_idle: Duration::from_secs(10),
		session_idle: Duration::from_secs(60),
		..PreviewConfig::default()
	}
}

/// Time of the first keyframe of `segment`: where it starts on the stream's
/// own clock. Absolute values include the MPEG-TS mux delay, so tests
/// compare differences between segments.
async fn first_keyframe(runner: &Runner, segment: &Path) -> Result<f64, RunError> {
	let keyframes = runner
		.keyframes(segment, 0, &Limits::timeout(LIMITS))
		.await?;
	assert!(
		!keyframes.is_empty(),
		"{} has no keyframe",
		segment.display()
	);
	Ok(keyframes[0])
}

/// Whether any process on this machine has an argument containing `text`.
/// Linux only: it reads `/proc`, which is the one way to look for a process
/// without spawning one.
fn process_with_argument(text: &str) -> bool {
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
				.any(|arg| arg.windows(text.len()).any(|w| w == text.as_bytes()))
		})
}

#[tokio::test]
async fn h264_aac_is_remuxed_on_keyframe_boundaries() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(
		&runner,
		dir.path(),
		"clip.mkv",
		12,
		H264,
		AAC,
		Some("0,3.5,7,9"),
	)
	.await
	.unwrap();
	let previews = PreviewManager::new(runner.clone(), config(), dir.path().join("previews"));

	let info = previews.start(&clip).await.unwrap();
	assert_eq!(info.mode, Mode::Remux);
	assert_eq!(info.segment_count, 4, "{info:?}");
	assert!((info.duration - 12.0).abs() < 0.1, "{info:?}");
	let playlist = previews.playlist(&info.id).unwrap();
	assert!(playlist.contains("#EXTINF:3.520000,\n0.ts"), "{playlist}");
	assert!(playlist.contains("#EXTINF:3.480000,\n1.ts"), "{playlist}");
	assert!(playlist.contains("#EXTINF:2.000000,\n2.ts"), "{playlist}");
	assert!(playlist.ends_with("3.ts\n#EXT-X-ENDLIST\n"), "{playlist}");

	// A seek into the middle: the run starts at that keyframe.
	let seg2 = previews.segment(&info.id, 2).await.unwrap();
	let seg3 = previews.segment(&info.id, 3).await.unwrap();
	let probe = runner.probe(&seg2, &Limits::timeout(LIMITS)).await.unwrap();
	assert_eq!(
		probe
			.streams_of(CodecType::Video)
			.next()
			.unwrap()
			.codec_name
			.as_deref(),
		Some("h264")
	);
	assert_eq!(
		probe
			.streams_of(CodecType::Audio)
			.next()
			.unwrap()
			.codec_name
			.as_deref(),
		Some("aac")
	);
	let (k2, k3) = (
		first_keyframe(&runner, &seg2).await.unwrap(),
		first_keyframe(&runner, &seg3).await.unwrap(),
	);
	assert!(
		(k3 - k2 - 2.0).abs() < 0.05,
		"segment 2 starts at 7.0, 3 at 9.0: {k2} {k3}"
	);

	// Back to the start: another run, on the same clock.
	let seg0 = previews.segment(&info.id, 0).await.unwrap();
	let seg1 = previews.segment(&info.id, 1).await.unwrap();
	let (k0, k1) = (
		first_keyframe(&runner, &seg0).await.unwrap(),
		first_keyframe(&runner, &seg1).await.unwrap(),
	);
	assert!((k1 - k0 - 3.52).abs() < 0.05, "{k0} {k1}");
	assert!((k2 - k0 - 7.0).abs() < 0.05, "{k0} {k2}");
	assert_ne!(seg0.parent(), seg2.parent(), "each run has its own folder");

	assert!(matches!(
		previews.segment(&info.id, 4).await,
		Err(SegmentError::OutOfRange(4))
	));
	assert!(previews.stop(&info.id).await);
	assert!(!dir.path().join("previews").join(&info.id).exists());
	assert!(previews.info(&info.id).is_none());
	assert!(!previews.stop(&info.id).await);
}

#[tokio::test]
async fn other_codecs_are_transcoded_on_the_grid() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(
		&runner,
		dir.path(),
		"clip.mkv",
		6,
		&["-c:v", "mpeg4", "-q:v", "5"],
		&["-c:a", "ac3", "-b:a", "96k"],
		None,
	)
	.await
	.unwrap();
	let previews = PreviewManager::new(runner.clone(), config(), dir.path().join("previews"));

	let info = previews.start(&clip).await.unwrap();
	assert_eq!(info.mode, Mode::Transcode);
	assert_eq!(info.segment_count, 3, "{info:?}");

	let seg1 = previews.segment(&info.id, 1).await.unwrap();
	let seg2 = previews.segment(&info.id, 2).await.unwrap();
	let probe = runner.probe(&seg1, &Limits::timeout(LIMITS)).await.unwrap();
	let video = probe.streams_of(CodecType::Video).next().unwrap();
	assert_eq!(video.codec_name.as_deref(), Some("h264"));
	assert_eq!(video.pix_fmt.as_deref(), Some("yuv420p"));
	let audio = probe.streams_of(CodecType::Audio).next().unwrap();
	assert_eq!(audio.codec_name.as_deref(), Some("aac"));
	assert_eq!(audio.channels, Some(2));
	let keyframes = runner
		.keyframes(&seg1, 0, &Limits::timeout(LIMITS))
		.await
		.unwrap();
	assert_eq!(
		keyframes.len(),
		1,
		"one keyframe per segment: {keyframes:?}"
	);
	let (k1, k2) = (keyframes[0], first_keyframe(&runner, &seg2).await.unwrap());
	assert!((k2 - k1 - 2.0).abs() < 0.05, "{k1} {k2}");

	// The same file again is the same session.
	let again = previews.start(&clip).await.unwrap();
	assert_eq!(again, info);
	previews.shutdown().await;
	assert!(!dir.path().join("previews").exists());
}

#[tokio::test]
async fn h264_with_other_audio_copies_the_video() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(
		&runner,
		dir.path(),
		"clip.mkv",
		5,
		H264,
		&["-c:a", "ac3"],
		Some("0,2.5"),
	)
	.await
	.unwrap();
	let previews = PreviewManager::new(runner.clone(), config(), dir.path().join("previews"));
	let info = previews.start(&clip).await.unwrap();
	assert_eq!(info.mode, Mode::TranscodeAudio);
	let seg = previews.segment(&info.id, 1).await.unwrap();
	let probe = runner.probe(&seg, &Limits::timeout(LIMITS)).await.unwrap();
	assert_eq!(
		probe
			.streams_of(CodecType::Audio)
			.next()
			.unwrap()
			.codec_name
			.as_deref(),
		Some("aac")
	);
}

#[tokio::test]
async fn idle_run_stops_and_idle_session_is_removed() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	// Long enough that the run is still going when the idle clock expires:
	// after the initial burst a run converts at twice real time.
	let clip = make_clip(
		&runner,
		dir.path(),
		"clip.mkv",
		90,
		H264,
		AAC,
		Some("expr:gte(t,n_forced*2)"),
	)
	.await
	.unwrap();
	let previews = PreviewManager::new(
		runner.clone(),
		PreviewConfig {
			run_idle: Duration::from_millis(500),
			session_idle: Duration::from_secs(2),
			..PreviewConfig::default()
		},
		dir.path().join("previews"),
	);
	let info = previews.start(&clip).await.unwrap();
	assert_eq!(info.mode, Mode::Remux);
	let session_dir = previews.session_dir(&info.id).unwrap();
	let marker = session_dir.to_string_lossy().into_owned();

	previews.segment(&info.id, 0).await.unwrap();
	assert!(previews.is_running(&info.id));
	assert!(
		process_with_argument(&marker),
		"ffmpeg writes into the session folder"
	);

	// Nobody fetches: ffmpeg stops, the segments stay, the session stays.
	tokio::time::sleep(Duration::from_millis(1200)).await;
	assert!(!previews.is_running(&info.id));
	assert!(!process_with_argument(&marker), "ffmpeg was stopped");
	assert!(previews.info(&info.id).is_some());
	assert!(session_dir.is_dir());

	// A new request starts a new run where needed.
	let seg = previews.segment(&info.id, 30).await.unwrap();
	assert!(seg.is_file());
	assert!(previews.is_running(&info.id));

	// Idle for the session's lifetime: everything is gone.
	tokio::time::sleep(Duration::from_millis(3500)).await;
	assert!(previews.info(&info.id).is_none());
	assert!(!session_dir.exists(), "segments were cleaned up");
	assert!(!process_with_argument(&marker), "ffmpeg was stopped");
}

#[tokio::test]
async fn unreadable_sources_are_errors() {
	let dir = tempfile::tempdir().unwrap();
	let previews = PreviewManager::new(Runner::default(), config(), dir.path().join("previews"));
	assert!(
		previews
			.start(&dir.path().join("missing.mkv"))
			.await
			.is_err()
	);
	assert!(previews.start(dir.path()).await.is_err());
	let text = dir.path().join("notes.txt");
	std::fs::write(&text, "not media").unwrap();
	assert!(previews.start(&text).await.is_err());
	assert!(matches!(
		previews.segment("nope", 0).await,
		Err(SegmentError::UnknownSession(_))
	));
}

#[tokio::test]
async fn a_temp_dir_manager_uses_a_random_folder_and_random_ids() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(&runner, dir.path(), "clip.mkv", 2, H264, AAC, None)
		.await
		.unwrap();
	let previews = PreviewManager::in_temp_dir(runner.clone(), config());
	let other = PreviewManager::in_temp_dir(runner, config());

	let info = previews.start(&clip).await.unwrap();
	assert_eq!(info.id.len(), 32, "{}", info.id);
	assert!(
		info.id.chars().all(|c| c.is_ascii_hexdigit()),
		"{}",
		info.id
	);
	let again = other.start(&clip).await.unwrap();
	assert_ne!(info.id, again.id, "ids are random, not a counter");

	let base = previews
		.session_dir(&info.id)
		.unwrap()
		.parent()
		.unwrap()
		.to_path_buf();
	let name = base.file_name().unwrap().to_string_lossy().into_owned();
	assert!(base.starts_with(std::env::temp_dir()), "{}", base.display());
	assert!(name.starts_with("muxmultitool-preview-"), "{name}");
	assert_ne!(name, format!("muxmultitool-preview-{}", std::process::id()));
	let other_base = other
		.session_dir(&again.id)
		.unwrap()
		.parent()
		.unwrap()
		.to_path_buf();
	assert_ne!(base, other_base, "every manager has a folder of its own");

	previews.shutdown().await;
	assert!(!base.exists(), "shutdown removes the folder");
	drop(other);
	assert!(!other_base.exists(), "a dropped manager removes its folder");
}
