//! A lossless merge with the real ffmpeg and ffprobe from `PATH`: two
//! generated clips pass the compatibility check and are joined without
//! re-encoding into a file as long as both together; a clip with other
//! audio parameters is caught by the check.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, ProbeResult, RunError, Runner};
use muxmultitool_core::merge::{self, Parameter, Part, command};

const TIMEOUT: Duration = Duration::from_secs(120);

/// `secs` seconds of test pattern and tone at `size`, H.264 + AAC at
/// `sample_rate` Hz.
async fn make_clip(
	runner: &Runner,
	path: &Path,
	secs: u32,
	size: &str,
	sample_rate: u32,
) -> Result<PathBuf, RunError> {
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input(format!("testsrc2=duration={secs}:size={size}:rate=25"))
		.option("-f", "lavfi")
		.input(format!("sine=frequency=440:duration={secs}"))
		.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
		.option("-ar", sample_rate.to_string())
		.output(path);
	runner.run(&cmd, &Limits::timeout(TIMEOUT), None).await?;
	Ok(path.to_path_buf())
}

async fn probe(runner: &Runner, path: &Path) -> Result<ProbeResult, RunError> {
	runner.probe(path, &Limits::timeout(TIMEOUT)).await
}

async fn part(runner: &Runner, path: &Path) -> Result<Part, RunError> {
	Ok(Part::from_probe(path, &probe(runner, path).await?))
}

#[tokio::test]
async fn two_clips_merge_into_one_as_long_as_both() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let first = make_clip(&runner, &dir.path().join("a.mp4"), 2, "320x240", 48_000)
		.await
		.unwrap();
	let second = make_clip(&runner, &dir.path().join("b.mp4"), 3, "320x240", 48_000)
		.await
		.unwrap();
	let parts = [
		part(&runner, &first).await.unwrap(),
		part(&runner, &second).await.unwrap(),
	];

	let check = merge::check(&parts, &[]).unwrap();
	assert!(check.can_merge(), "{}", check.reason());
	assert_eq!(check.rows.len(), 2);
	assert_eq!(check.rows[0].cells[0].as_deref(), Some("h264 320×240 25"));
	assert_eq!(check.rows[1].cells[1].as_deref(), Some("aac mono 48 kHz"));
	let total = check.total_duration.expect("both clips have a duration");
	assert!(
		(total.as_secs_f64() - 5.0).abs() < 0.2,
		"the sum of the inputs is about 5 s, not {total:?}"
	);

	let list = dir.path().join("list.txt");
	std::fs::write(&list, command::concat_list(&parts)).unwrap();
	let chapters = dir.path().join("chapters.ffmeta");
	std::fs::write(&chapters, command::chapters(&parts).unwrap()).unwrap();
	let output = dir.path().join("merged.mp4");
	let cmd = command::merge(&list, Some(&chapters), &[], &output);
	runner
		.run(&cmd, &Limits::timeout(TIMEOUT), None)
		.await
		.unwrap();

	let merged = probe(&runner, &output).await.unwrap();
	let duration = merged.duration_secs().unwrap();
	// AAC adds a few milliseconds of priming per part; the picture is exact.
	assert!(
		(duration - total.as_secs_f64()).abs() < 0.2,
		"the output lasts {duration} s, the inputs {total:?}"
	);
	let video = merged.streams_of(CodecType::Video).next().unwrap();
	assert_eq!(video.codec_name.as_deref(), Some("h264"));
	assert_eq!(video.nb_frames, Some(125), "25 fps × 5 s, no frame dropped");
	let audio = merged.streams_of(CodecType::Audio).next().unwrap();
	assert_eq!(audio.codec_name.as_deref(), Some("aac"));
	assert_eq!(audio.sample_rate, Some(48_000));
	let titles: Vec<&str> = merged
		.chapters
		.iter()
		.filter_map(|c| c.tags.get("title").map(String::as_str))
		.collect();
	assert_eq!(titles, ["a", "b"]);
	assert!((merged.chapters[1].start_time.unwrap() - 2.0).abs() < 0.1);
}

#[tokio::test]
async fn a_clip_with_other_audio_is_caught_before_the_merge() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let first = make_clip(&runner, &dir.path().join("a.mp4"), 1, "320x240", 48_000)
		.await
		.unwrap();
	let other = make_clip(&runner, &dir.path().join("b.mp4"), 1, "160x120", 22_050)
		.await
		.unwrap();
	let parts = [
		part(&runner, &first).await.unwrap(),
		part(&runner, &other).await.unwrap(),
	];

	let check = merge::check(&parts, &[]).unwrap();
	assert!(!check.can_merge());
	let found: Vec<(u32, Parameter)> = check.blocking().map(|m| (m.stream, m.parameter)).collect();
	assert_eq!(
		found,
		[(0, Parameter::Resolution), (1, Parameter::SampleRate)]
	);
	assert_eq!(
		check.reason(),
		"resolution differs (b.mp4): 160×120 instead of 320×240; sample rate differs (b.mp4): 22.05 kHz instead of 48 kHz"
	);
	// Leaving the audio out fixes that half; the picture still blocks.
	let without_audio = merge::check(&parts, &[1]).unwrap();
	assert_eq!(without_audio.blocking().count(), 1);
	assert!(without_audio.rows[1].dropped);
}
