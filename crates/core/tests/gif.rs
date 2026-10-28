//! GIF conversion in both directions with the real ffmpeg and ffprobe from
//! `PATH`: a generated clip becomes a GIF within its limits, and that GIF
//! becomes an MP4 and a `WebM` with the encoders the build has.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, ProbeResult, RunError, Runner};
use muxmultitool_core::gif::{
	self, Limit, Quality, Request, Source, VideoEncoder, VideoFormat, command,
};

const TIMEOUT: Duration = Duration::from_secs(120);

/// Four seconds of test pattern and tone, 640×360 at 25 fps, H.264 + AAC.
async fn make_clip(runner: &Runner, dir: &Path) -> Result<PathBuf, RunError> {
	let clip = dir.join("clip.mp4");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=duration=4:size=640x360:rate=25")
		.option("-f", "lavfi")
		.input("sine=frequency=440:duration=4")
		.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
		.output(&clip);
	runner.run(&cmd, &Limits::timeout(TIMEOUT), None).await?;
	Ok(clip)
}

async fn probe(runner: &Runner, path: &Path) -> Result<ProbeResult, RunError> {
	runner.probe(path, &Limits::timeout(TIMEOUT)).await
}

#[tokio::test]
async fn a_range_becomes_a_gif_and_the_gif_becomes_videos() {
	let runner = Runner::default();
	let caps = muxmultitool_core::ffmpeg::capabilities::detect(&runner)
		.await
		.expect("ffmpeg reports its capabilities");
	assert_eq!(gif::missing_for_gif(&caps), [] as [&str; 0]);
	let dir = tempfile::tempdir().unwrap();
	let clip = make_clip(&runner, dir.path()).await.unwrap();
	let source =
		Source::from_probe(&probe(&runner, &clip).await.unwrap()).expect("the clip has video");
	assert_eq!((source.width, source.height), (640, 360));

	// Video → GIF: 1.0 s to 3.0 s at 10 fps, 320 px wide.
	let request = Request {
		start: Duration::from_secs(1),
		end: Duration::from_secs(3),
		fps: 10,
		width: 320,
		quality: Quality::Balanced,
		play_once: false,
	};
	let plan = gif::plan(&request, &source).unwrap();
	assert_eq!(
		(plan.width, plan.height, plan.fps, plan.frames),
		(320, 180, 10, 20)
	);
	assert_eq!(plan.duration, Duration::from_secs(2));
	assert!(plan.within(&gif::Limits::default()), "{plan:?}");

	let gif_path = dir.path().join("clip.gif");
	runner
		.run(
			&plan.command(&clip, &gif_path),
			&Limits::timeout(TIMEOUT),
			None,
		)
		.await
		.expect("ffmpeg makes the gif");
	let made = probe(&runner, &gif_path).await.unwrap();
	assert_eq!(made.format.format_name, "gif");
	let video = made.streams_of(CodecType::Video).next().unwrap();
	assert_eq!((video.width, video.height), (Some(320), Some(180)));
	assert_eq!(video.nb_frames, Some(20));
	assert!(
		made.duration_secs().is_some_and(|d| (d - 2.0).abs() < 0.15),
		"{:?}",
		made.format
	);
	let actual = std::fs::metadata(&gif_path).unwrap().len();
	assert!(
		actual > 1000 && actual <= plan.estimated_bytes,
		"the estimate ({}) is an upper bound for a static test pattern ({actual})",
		plan.estimated_bytes
	);

	// GIF → MP4 (H.264) and WebM (VP9 or VP8), whichever the build has.
	for format in [VideoFormat::Mp4, VideoFormat::Webm] {
		let Some(encoder) = format.encoder(&caps) else {
			eprintln!(
				"skipping {format}: this ffmpeg build lacks {:?}",
				format.encoders()
			);
			continue;
		};
		let output = dir.path().join(format!("back.{}", format.muxer()));
		runner
			.run(
				&command::gif_to_video(&gif_path, &output, encoder),
				&Limits::timeout(TIMEOUT),
				None,
			)
			.await
			.unwrap_or_else(|e| panic!("ffmpeg converts the gif to {format}: {e}"));
		let back = probe(&runner, &output).await.unwrap();
		let expected_codec = match encoder {
			VideoEncoder::X264 => "h264",
			VideoEncoder::Vp9 => "vp9",
			VideoEncoder::Vp8 => "vp8",
		};
		let video = back.streams_of(CodecType::Video).next().unwrap();
		assert_eq!(video.codec_name.as_deref(), Some(expected_codec));
		assert_eq!((video.width, video.height), (Some(320), Some(180)));
		assert_eq!(back.streams_of(CodecType::Audio).count(), 0);
		assert!(
			back.format.format_name.contains(format.muxer()),
			"{format}: {}",
			back.format.format_name
		);
		assert!(
			back.duration_secs().is_some_and(|d| (d - 2.0).abs() < 0.15),
			"{format}: {:?}",
			back.format
		);
	}
}

#[tokio::test]
async fn a_request_over_the_limits_is_refused_before_ffmpeg_runs() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clip = make_clip(&runner, dir.path()).await.unwrap();
	let source = Source::from_probe(&probe(&runner, &clip).await.unwrap()).unwrap();
	let plan = gif::plan(
		&Request {
			start: Duration::ZERO,
			end: Duration::from_secs(4),
			fps: 25,
			width: 640,
			quality: Quality::Best,
			play_once: false,
		},
		&source,
	)
	.unwrap();
	let limits = gif::Limits {
		max_duration: Duration::from_secs(3),
		max_width: 480,
		max_height: None,
		max_bytes: 2 * gif::MB,
	};
	let violations = plan.violations(&limits);
	let limits_hit: Vec<Limit> = violations.iter().map(|v| v.limit).collect();
	assert_eq!(limits_hit, [Limit::Duration, Limit::Width, Limit::Size]);
	for violation in &violations {
		assert!(
			violation.message.contains("over the"),
			"{}",
			violation.message
		);
	}
	assert!(!dir.path().join("clip.gif").exists());
}
