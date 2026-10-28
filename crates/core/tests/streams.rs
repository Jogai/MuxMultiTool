//! Stream copies with the real ffmpeg from `PATH`: the planned commands
//! run on small sample files and the outputs are probed for exactly the
//! chosen streams. Also checks that a combination the plan calls blocking
//! really is refused by ffmpeg, so the compatibility table matches the
//! build.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, ProbeResult, Runner};
use muxmultitool_core::streams::{Container, Selection, plan};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Two-second samples: an MKV with video, audio and a SubRip subtitle, and
/// an MP4 with video and audio.
struct Samples {
	_dir: tempfile::TempDir,
	root: PathBuf,
	mkv: PathBuf,
	mp4: PathBuf,
	runner: Runner,
	limits: Limits,
}

impl Samples {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let root = dir.path().to_path_buf();
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(60));
		let subs = root.join("subs.srt");
		std::fs::write(&subs, "1\n00:00:00,000 --> 00:00:01,000\nhello\n\n")?;
		let mkv = root.join("clip.mkv");
		let cmd = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("testsrc2=duration=2:size=64x64:rate=10")
			.option("-f", "lavfi")
			.input("sine=frequency=440:duration=2")
			.input(&subs)
			.args(["-map", "0", "-map", "1", "-map", "2"])
			.option("-c:v", "libx264")
			.option("-preset", "ultrafast")
			.option("-pix_fmt", "yuv420p")
			.option("-c:a", "aac")
			.option("-c:s", "srt")
			.option("-metadata:s:a:0", "language=eng")
			.output(&mkv);
		runner.run(&cmd, &limits, None).await?;
		let mp4 = root.join("clip.mp4");
		let cmd = FfmpegCommand::new()
			.input(&mkv)
			.args(["-map", "0:0", "-map", "0:1"])
			.option("-c", "copy")
			.output(&mp4);
		runner.run(&cmd, &limits, None).await?;
		Ok(Self {
			_dir: dir,
			root,
			mkv,
			mp4,
			runner,
			limits,
		})
	}

	async fn probe(&self, path: &Path) -> Result<ProbeResult> {
		Ok(self.runner.probe(path, &self.limits).await?)
	}

	/// Plans `selection` from `input` to `output` (the container follows
	/// the extension), runs the command and probes the result.
	async fn copy(
		&self,
		input: &Path,
		selection: &Selection,
		output: &str,
	) -> Result<(muxmultitool_core::streams::Plan, ProbeResult)> {
		let output = self.root.join(output);
		let container = Container::from_extension(&output).ok_or("no container")?;
		let plan = plan(&self.probe(input).await?, selection, container)?;
		assert!(plan.can_run(), "{:?}", plan.issues);
		self.runner
			.run(&plan.command(input, &output), &self.limits, None)
			.await?;
		let probe = self.probe(&output).await?;
		Ok((plan, probe))
	}
}

/// `(codec_type, codec_name)` of every stream, in container order.
fn streams(probe: &ProbeResult) -> Vec<(CodecType, String)> {
	probe
		.streams
		.iter()
		.map(|s| (s.codec_type, s.codec_name.clone().unwrap_or_default()))
		.collect()
}

fn video_audio_subs() -> Vec<(CodecType, String)> {
	vec![
		(CodecType::Video, "h264".into()),
		(CodecType::Audio, "aac".into()),
		(CodecType::Subtitle, "subrip".into()),
	]
}

#[tokio::test]
async fn remux_copies_every_stream_into_the_new_container() {
	let samples = Samples::new().await.unwrap();
	assert_eq!(
		streams(&samples.probe(&samples.mkv).await.unwrap()),
		video_audio_subs()
	);

	// MP4 → MKV, the most common case: both streams, same codecs, no re-encode.
	let (plan, out) = samples
		.copy(&samples.mp4, &Selection::all(), "remuxed.mkv")
		.await
		.unwrap();
	assert!(plan.keeps_everything());
	assert!(
		out.format.format_name.starts_with("matroska"),
		"{}",
		out.format.format_name
	);
	assert_eq!(streams(&out), video_audio_subs()[..2]);
	let source = samples.probe(&samples.mp4).await.unwrap();
	assert_eq!(out.streams[0].width, source.streams[0].width);
	assert_eq!(out.streams[1].sample_rate, source.streams[1].sample_rate);

	// MKV → MKV keeps the subtitle and the language tag as well.
	let (_, out) = samples
		.copy(&samples.mkv, &Selection::all(), "same.mkv")
		.await
		.unwrap();
	assert_eq!(streams(&out), video_audio_subs());
	assert_eq!(out.streams[1].language(), Some("eng"));
}

#[tokio::test]
async fn keep_and_drop_leave_exactly_the_chosen_streams() {
	let samples = Samples::new().await.unwrap();
	let (_, kept) = samples
		.copy(&samples.mkv, &Selection::keep([0, 2]), "kept.mkv")
		.await
		.unwrap();
	assert_eq!(
		streams(&kept),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Subtitle, "subrip".into())
		]
	);
	let (_, dropped) = samples
		.copy(&samples.mkv, &Selection::drop([0]), "dropped.mka")
		.await
		.unwrap();
	assert_eq!(
		streams(&dropped),
		[
			(CodecType::Audio, "aac".into()),
			(CodecType::Subtitle, "subrip".into())
		]
	);
}

#[tokio::test]
async fn video_only_output_has_one_video_stream() {
	let samples = Samples::new().await.unwrap();
	let (plan, out) = samples
		.copy(&samples.mkv, &Selection::video_only(), "video.mp4")
		.await
		.unwrap();
	assert_eq!(plan.kept().collect::<Vec<_>>(), [0]);
	assert_eq!(streams(&out), [(CodecType::Video, "h264".into())]);
	assert!(
		out.format.format_name.contains("mp4"),
		"{}",
		out.format.format_name
	);
}

#[tokio::test]
async fn a_blocked_combination_really_fails_in_ffmpeg() {
	let samples = Samples::new().await.unwrap();
	let probe = samples.probe(&samples.mkv).await.unwrap();
	let plan = plan(&probe, &Selection::all(), Container::Mp4).unwrap();
	assert!(!plan.can_run());
	assert_eq!(
		plan.blocking_messages(),
		["MP4 cannot hold subrip subtitles (stream 2); drop stream 2 or choose MKV"]
	);
	let output = samples.root.join("blocked.mp4");
	let error = samples
		.runner
		.run(&plan.command(&samples.mkv, &output), &samples.limits, None)
		.await
		.unwrap_err();
	assert!(
		error.to_string().contains("subrip"),
		"ffmpeg should name the codec: {error}"
	);
}
