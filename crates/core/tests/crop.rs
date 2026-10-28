//! Lossless crop with the real ffmpeg and ffprobe from `PATH`: a generated
//! letterboxed clip in H.264 and in H.265 has its bars detected and hidden,
//! and the output shows the cropped size while its video packets are the
//! same bytes. A clip in another codec is refused.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::crop::{self, Codec, Coded, Crop, Source, Unsupported, command, detect};
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, ProbeResult, RunError, Runner, Tool};
use tokio::sync::mpsc;

const TIMEOUT: Duration = Duration::from_secs(120);

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Three seconds of 640×240 test pattern inside 60 px black bars top and
/// bottom (640×360), with a tone, encoded with `encoder`.
async fn make_letterboxed(
	runner: &Runner,
	dir: &Path,
	name: &str,
	encoder: &str,
) -> std::result::Result<PathBuf, RunError> {
	let clip = dir.join(name);
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=duration=3:size=640x240:rate=25")
		.option("-f", "lavfi")
		.input("sine=frequency=440:duration=3")
		.option("-vf", "pad=640:360:0:60:black")
		.option("-c:v", encoder);
	let cmd = if encoder.starts_with("libx") {
		cmd.option("-preset", "ultrafast")
	} else {
		cmd
	};
	let cmd = cmd
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
		.output(&clip);
	runner.run(&cmd, &Limits::timeout(TIMEOUT), None).await?;
	Ok(clip)
}

async fn probe(runner: &Runner, path: &Path) -> std::result::Result<ProbeResult, RunError> {
	runner.probe(path, &Limits::timeout(TIMEOUT)).await
}

/// Runs the detect command and collects what ffmpeg wrote to stderr.
async fn run_detect(
	runner: &Runner,
	cmd: &FfmpegCommand,
) -> std::result::Result<Vec<String>, RunError> {
	let (tx, mut rx) = mpsc::channel(256);
	let collect = async {
		let mut lines = Vec::new();
		while let Some(line) = rx.recv().await {
			lines.push(line);
		}
		lines
	};
	let limits = Limits::timeout(TIMEOUT);
	let (run, lines) = tokio::join!(runner.run_with_log(cmd, &limits, None, Some(tx)), collect);
	run?;
	Ok(lines)
}

async fn read_coded(runner: &Runner, path: &Path, source: &Source) -> Result<Coded> {
	let args = detect::coded_probe_args(path, source.stream_index);
	let args: Vec<&str> = args.iter().map(String::as_str).collect();
	let csv = runner
		.capture(Tool::Ffprobe, &args, &Limits::timeout(TIMEOUT))
		.await?;
	detect::parse_coded(&csv).ok_or_else(|| format!("no coded size in {csv:?}").into())
}

/// Detects the bars of `clip`, crops it to `output` and checks the result.
async fn detect_and_crop(
	runner: &Runner,
	clip: &Path,
	output: &Path,
	expected: Codec,
) -> Result<()> {
	let caps = muxmultitool_core::ffmpeg::capabilities::detect(runner).await?;
	let source = Source::from_probe(&probe(runner, clip).await?)?;
	assert_eq!(source.codec, expected);
	assert_eq!((source.width, source.height), (640, 360));
	assert_eq!((source.unit.x, source.unit.y), (2, 2));
	assert_eq!(source.check_build(&caps), Ok(()));

	// The header's own window: the encoder pads 360 lines to 368.
	let coded = read_coded(runner, clip, &source).await?;
	assert_eq!((coded.width, coded.height), (640, 368), "{coded:?}");
	assert_eq!(
		coded.crop,
		Crop {
			bottom: 8,
			..Crop::default()
		},
		"{coded:?}"
	);

	// Detection finds the 60 px bars from the keyframe at the start.
	let lines = run_detect(
		runner,
		&command::detect(clip, &source, Duration::from_millis(500)),
	)
	.await?;
	let found = detect::parse_cropdetect(lines.iter().map(String::as_str), &source)
		.ok_or_else(|| format!("cropdetect reports no picture in {lines:#?}"))?;
	assert!(found.frames >= 1, "{found:?}");
	assert_eq!(
		found.crop,
		Crop {
			top: 60,
			bottom: 60,
			left: 0,
			right: 0,
		},
		"{lines:#?}"
	);

	// The crop: an odd request snaps, the plan sizes the result.
	let plan = crop::plan(
		&source,
		Crop {
			top: 61,
			bottom: 60,
			left: 0,
			right: 0,
		},
	)?;
	assert_eq!(plan.crop, found.crop);
	assert_eq!((plan.width, plan.height), (640, 240));
	runner
		.run(
			&command::crop(clip, output, &plan, &coded),
			&Limits::timeout(TIMEOUT),
			None,
		)
		.await?;

	// The output shows the cropped size, keeps every stream, and holds the
	// same video bytes: the packets were copied, not re-encoded.
	let cropped = probe(runner, output).await?;
	let video = cropped
		.streams_of(CodecType::Video)
		.next()
		.ok_or("no video stream in the output")?;
	assert_eq!(
		video.codec_name.as_deref(),
		Some(source_codec_name(expected))
	);
	assert_eq!((video.width, video.height), (Some(640), Some(240)));
	assert_eq!(cropped.streams_of(CodecType::Audio).count(), 1);
	let after = read_coded(runner, output, &source).await?;
	assert_eq!((after.width, after.height), (640, 368));
	assert_eq!(after.crop, plan.header_crop(&coded));
	let in_sizes = runner.packet_sizes(clip, &Limits::timeout(TIMEOUT)).await?;
	let out_sizes = runner
		.packet_sizes(output, &Limits::timeout(TIMEOUT))
		.await?;
	let in_bytes = in_sizes
		.get(&source.stream_index)
		.copied()
		.ok_or("no input video packets")?;
	let out_bytes = out_sizes
		.get(&source.stream_index)
		.copied()
		.ok_or("no output video packets")?;
	let diff = in_bytes.abs_diff(out_bytes);
	assert!(
		diff < in_bytes / 100,
		"video packets are the same size apart from the header: {in_bytes} vs {out_bytes}"
	);
	Ok(())
}

fn source_codec_name(codec: Codec) -> &'static str {
	match codec {
		Codec::H264 => "h264",
		Codec::Hevc => "hevc",
	}
}

#[tokio::test]
async fn h264_bars_are_detected_and_hidden_without_re_encoding() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clip = make_letterboxed(&runner, dir.path(), "bars.mp4", "libx264")
		.await
		.unwrap();
	detect_and_crop(
		&runner,
		&clip,
		&dir.path().join("bars.cropped.mp4"),
		Codec::H264,
	)
	.await
	.unwrap();
}

#[tokio::test]
async fn hevc_bars_are_detected_and_hidden_without_re_encoding() {
	let runner = Runner::default();
	let caps = muxmultitool_core::ffmpeg::capabilities::detect(&runner)
		.await
		.unwrap();
	if !caps.has_encoder("libx265") {
		eprintln!("skipping: this ffmpeg build lacks libx265 to make an H.265 clip");
		return;
	}
	let dir = tempfile::tempdir().unwrap();
	let clip = make_letterboxed(&runner, dir.path(), "bars.mkv", "libx265")
		.await
		.unwrap();
	detect_and_crop(
		&runner,
		&clip,
		&dir.path().join("bars.cropped.mkv"),
		Codec::Hevc,
	)
	.await
	.unwrap();
}

#[tokio::test]
async fn another_codec_is_refused_with_an_explanation() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clip = make_letterboxed(&runner, dir.path(), "other.mkv", "mpeg4")
		.await
		.unwrap();
	let error = Source::from_probe(&probe(&runner, &clip).await.unwrap()).unwrap_err();
	assert_eq!(
		error,
		Unsupported::Codec {
			name: "mpeg4".into()
		}
	);
	assert_eq!(
		error.to_string(),
		"lossless crop needs H.264 or H.265; this video is mpeg4"
	);
}
