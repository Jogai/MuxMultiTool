//! Combining sources with the real ffmpeg from `PATH`: the planned
//! commands run on small sample files and the outputs are probed for
//! exactly the chosen tracks, in order, with the language, title and flags
//! each should carry. Also checks that a track the plan calls blocking
//! really is refused by ffmpeg.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::combine::{self, Plan, Source, Track, Warning};
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, ProbeResult, Runner};
use muxmultitool_core::streams::Container;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Three sources: A, a two-second MKV with video and English audio; B, a
/// five-second MKV with video, French audio titled "French" and an English
/// subtitle titled "English"; C, a SubRip file.
struct Samples {
	_dir: tempfile::TempDir,
	root: PathBuf,
	a: PathBuf,
	b: PathBuf,
	srt: PathBuf,
	runner: Runner,
	limits: Limits,
}

impl Samples {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let root = dir.path().to_path_buf();
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(60));
		let srt = root.join("subs.srt");
		std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,000\nhello\n\n")?;
		let a = root.join("a.mkv");
		let cmd = clip(2, None)
			.args(["-map", "0", "-map", "1"])
			.option("-metadata:s:a:0", "language=eng")
			.output(&a);
		runner.run(&cmd, &limits, None).await?;
		let b = root.join("b.mkv");
		let cmd = clip(5, Some(&srt))
			.args(["-map", "0", "-map", "1", "-map", "2"])
			.option("-metadata:s:a:0", "language=fre")
			.option("-metadata:s:a:0", "title=French")
			.option("-metadata:s:s:0", "language=eng")
			.option("-metadata:s:s:0", "title=English")
			.output(&b);
		runner.run(&cmd, &limits, None).await?;
		Ok(Self {
			_dir: dir,
			root,
			a,
			b,
			srt,
			runner,
			limits,
		})
	}

	async fn probe(&self, path: &Path) -> Result<ProbeResult> {
		Ok(self.runner.probe(path, &self.limits).await?)
	}

	async fn source(&self, path: &Path) -> Result<Source> {
		Ok(Source::from_probe(path, &self.probe(path).await?))
	}

	/// Plans `tracks` from `sources` to `output` (the container follows
	/// the extension), runs the command and probes the result.
	async fn combine(
		&self,
		sources: &[Source],
		tracks: &[Track],
		output: &str,
	) -> Result<(Plan, ProbeResult)> {
		let output = self.root.join(output);
		let container = Container::from_extension(&output).ok_or("no container")?;
		let plan = combine::plan(sources, tracks, container)?;
		assert!(plan.can_run(), "{:?}", plan.issues);
		self.runner
			.run(&plan.command(&output), &self.limits, None)
			.await?;
		let probe = self.probe(&output).await?;
		Ok((plan, probe))
	}
}

/// `secs` seconds of test pattern and tone (inputs 0 and 1), plus `subs`
/// as input 2 when given, encoded as H.264 + AAC (+ SubRip); the mapping
/// and the output are still to add.
fn clip(secs: u32, subs: Option<&Path>) -> FfmpegCommand {
	let mut cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input(format!("testsrc2=duration={secs}:size=64x64:rate=10"))
		.option("-f", "lavfi")
		.input(format!("sine=frequency=440:duration={secs}"));
	if let Some(subs) = subs {
		cmd = cmd.input(subs).option("-c:s", "srt");
	}
	cmd.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
}

/// `(codec_type, codec_name)` of every stream, in container order.
fn streams(probe: &ProbeResult) -> Vec<(CodecType, String)> {
	probe
		.streams
		.iter()
		.map(|s| (s.codec_type, s.codec_name.clone().unwrap_or_default()))
		.collect()
}

#[tokio::test]
async fn subtitles_are_copied_from_another_file_with_their_metadata() {
	let samples = Samples::new().await.unwrap();
	let sources = [
		samples.source(&samples.a).await.unwrap(),
		samples.source(&samples.b).await.unwrap(),
	];
	let (plan, out) = samples
		.combine(
			&sources,
			&[Track::new(0, 0), Track::new(0, 1), Track::new(1, 2)],
			"a-with-subs.mkv",
		)
		.await
		.unwrap();
	assert!(plan.tracks.iter().all(|t| t.edits.is_empty()));
	// Only subtitles come from B, so its longer duration is not compared.
	assert_eq!(plan.warnings, Vec::<Warning>::new());
	assert_eq!(
		streams(&out),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Audio, "aac".into()),
			(CodecType::Subtitle, "subrip".into()),
		]
	);
	assert_eq!(out.streams[1].language(), Some("eng"));
	assert_eq!(out.streams[2].language(), Some("eng"));
	assert_eq!(
		out.streams[2].tags.get("title").map(String::as_str),
		Some("English")
	);
	assert!(
		(out.duration_secs().unwrap() - 2.0).abs() < 0.2,
		"the output lasts as long as A: {:?}",
		out.duration_secs()
	);
}

#[tokio::test]
async fn video_from_one_file_with_audio_and_subs_from_others_makes_one_mkv() {
	let samples = Samples::new().await.unwrap();
	let sources = [
		samples.source(&samples.a).await.unwrap(),
		samples.source(&samples.b).await.unwrap().with_offset(0.5),
		samples.source(&samples.srt).await.unwrap(),
	];
	let tracks = [
		Track::new(0, 0),
		Track::new(1, 1).title("Atmos").as_default(true),
		Track::new(2, 0)
			.language("eng")
			.title("English")
			.as_forced(true),
	];
	let (plan, out) = samples
		.combine(&sources, &tracks, "movie.mkv")
		.await
		.unwrap();

	// A lasts 2 s, B 5 s: the plan warns, and the output follows B.
	assert_eq!(plan.warnings.len(), 1, "{:?}", plan.warnings);
	let warning = plan.warnings[0].to_string();
	assert!(
		warning.starts_with("durations differ: A (a.mkv) 0:00:02.0") && warning.ends_with("s)"),
		"{warning}"
	);
	assert!(
		matches!(
			&plan.warnings[0],
			Warning::DurationsDiffer { other, .. }
				if other.0 == "B (b.mkv)" && other.1 > Duration::from_secs(4)
		),
		"{:?}",
		plan.warnings[0]
	);
	let expected = plan.expected_duration().unwrap().as_secs_f64();
	assert!(
		(expected - 5.5).abs() < 0.2,
		"5 s plus the offset: {expected}"
	);

	assert_eq!(
		streams(&out),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Audio, "aac".into()),
			(CodecType::Subtitle, "subrip".into()),
		]
	);
	let audio = &out.streams[1];
	assert_eq!(audio.language(), Some("fre"), "carried over from B");
	assert_eq!(
		audio.tags.get("title").map(String::as_str),
		Some("Atmos"),
		"set by the track"
	);
	assert!(audio.disposition.default);
	let start = audio.start_time.unwrap_or_default();
	assert!((start - 0.5).abs() < 0.1, "B is shifted by 0.5 s: {start}");
	let subs = &out.streams[2];
	assert_eq!(subs.language(), Some("eng"));
	assert_eq!(subs.tags.get("title").map(String::as_str), Some("English"));
	assert!(subs.disposition.forced);
	let duration = out.duration_secs().unwrap();
	assert!(
		(duration - 5.5).abs() < 0.3,
		"the output lasts as long as the longest track: {duration}"
	);
}

#[tokio::test]
async fn a_blocked_track_really_fails_in_ffmpeg() {
	let samples = Samples::new().await.unwrap();
	let sources = [
		samples.source(&samples.a).await.unwrap(),
		samples.source(&samples.srt).await.unwrap(),
	];
	let plan = combine::plan(
		&sources,
		&[Track::new(0, 0), Track::new(1, 0)],
		Container::Mp4,
	)
	.unwrap();
	assert!(!plan.can_run());
	assert_eq!(
		plan.blocking_messages(),
		["MP4 cannot hold subrip subtitles (track 2, B:#0); remove the track or choose MKV"]
	);
	let output = samples.root.join("blocked.mp4");
	let error = samples
		.runner
		.run(&plan.command(&output), &samples.limits, None)
		.await
		.unwrap_err();
	assert!(
		error.to_string().contains("subrip"),
		"ffmpeg should name the codec: {error}"
	);
}
