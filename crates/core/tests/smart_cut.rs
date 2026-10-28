//! Smart cuts with the real ffmpeg and ffprobe from `PATH`, on H.264 and
//! HEVC clips: the output decodes end to end without an error, holds
//! exactly the frames from the chosen start to the chosen end, copies the
//! body bit for bit, and keeps the audio in sync across the joins. The
//! audio carries a beep at the start of every second, so its position in
//! the output can be measured against the video's.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::cut::smart::{self, PieceKind};
use muxmultitool_core::cut::{self, CutMode, Export, Request, Segment, SegmentMode, Source};
use muxmultitool_core::ffmpeg::probe::Packet;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner, Tool};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const TIMEOUT: Duration = Duration::from_secs(300);
/// The clips are 25 fps with a keyframe every second.
const FPS: f64 = 25.0;
/// How far the packets are read around a cut.
const WINDOW: f64 = 5.0;
/// How far a beep may be from where the video says it should be.
const SYNC_TOLERANCE: f64 = 0.015;

fn limits() -> Limits {
	Limits::timeout(TIMEOUT)
}

/// Ten seconds of test pattern with three B-frames and a keyframe every
/// second, plus a 880 Hz beep in the first 100 ms of every second as AAC:
/// as H.264 in MP4 and MPEG-TS, and as HEVC in MP4.
struct Clips {
	h264: PathBuf,
	h264_ts: PathBuf,
	hevc: PathBuf,
}

async fn make_clips(runner: &Runner, dir: &Path) -> Result<Clips> {
	let h264 = dir.join("h264.mp4");
	let hevc = dir.join("hevc.mp4");
	let h264_ts = dir.join("h264.ts");
	let inputs = |cmd: FfmpegCommand| {
		cmd.option("-f", "lavfi")
			.input("testsrc2=duration=10:size=320x240:rate=25")
			.option("-f", "lavfi")
			.input(r"aevalsrc=sin(2*PI*880*t)*lt(mod(t\,1)\,0.1):s=48000:d=10")
	};
	let cmd = inputs(FfmpegCommand::new())
		.option("-c:v", "libx264")
		.option("-preset", "veryfast")
		.option("-bf", "3")
		.option("-g", "25")
		.option("-keyint_min", "25")
		.option("-sc_threshold", "0")
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
		.output(&h264);
	runner.run(&cmd, &limits(), None).await?;
	let cmd = inputs(FfmpegCommand::new())
		.option("-c:v", "libx265")
		.option("-preset", "veryfast")
		.option(
			"-x265-params",
			"bframes=3:keyint=25:min-keyint=25:scenecut=0:log-level=error",
		)
		.option("-pix_fmt", "yuv420p")
		.option("-tag:v", "hvc1")
		.option("-c:a", "aac")
		.output(&hevc);
	runner.run(&cmd, &limits(), None).await?;
	let cmd = FfmpegCommand::new()
		.input(&h264)
		.option("-c", "copy")
		.output(&h264_ts);
	runner.run(&cmd, &limits(), None).await?;
	Ok(Clips {
		h264,
		h264_ts,
		hevc,
	})
}

fn request(segments: &[(f64, f64)], export: Export, output: &Path) -> Request {
	Request {
		segments: segments.iter().map(|(s, e)| Segment::new(*s, *e)).collect(),
		segment_mode: SegmentMode::Keep,
		cut_mode: CutMode::Smart,
		export,
		output: output.to_path_buf(),
	}
}

/// Plans `request` on `input` the way the server does: the packets around
/// the cuts, the whole range when that is not enough.
async fn plan_on(
	runner: &Runner,
	input: &Path,
	request: &Request,
	scratch: &Path,
) -> Result<smart::Plan> {
	let probe = runner.probe(input, &limits()).await?;
	let source = Source::from_probe(&probe, input)?;
	let base = cut::plan(request, &source, &[])?;
	let video = smart::check(&probe, &source, |_| true)?;
	let streams = smart::copied_streams(&probe);
	let windows = smart::windows(&base, &source, WINDOW);
	let packets = runner
		.packets_in(input, video.stream, &windows, &limits())
		.await?;
	let plan = smart::plan(
		&base,
		&source,
		video.clone(),
		streams.clone(),
		&packets,
		Some(WINDOW),
		scratch,
	);
	Ok(match plan {
		Err(smart::PlanError::ReadWholeRange { .. }) => {
			let all = runner
				.packets_in(
					input,
					video.stream,
					&smart::whole_range_windows(&base, &source),
					&limits(),
				)
				.await?;
			smart::plan(&base, &source, video, streams, &all, None, scratch)?
		}
		other => other?,
	})
}

/// Runs the plan's commands as the job would, scratch folder included.
async fn export(runner: &Runner, input: &Path, plan: &smart::Plan) -> Result<()> {
	std::fs::create_dir_all(&plan.scratch_dir)?;
	for command in plan.commands(input) {
		runner.run(&command, &limits(), None).await?;
	}
	std::fs::remove_dir_all(&plan.scratch_dir)?;
	for output in plan.outputs() {
		assert!(output.is_file(), "{} was not written", output.display());
	}
	Ok(())
}

/// Decodes `path` end to end; whatever ffmpeg says at error level.
async fn decode_errors(runner: &Runner, path: &Path) -> Result<String> {
	let cmd = FfmpegCommand::new()
		.option("-v", "error")
		.input(path)
		.option("-f", "null")
		.output("-");
	Ok(runner.run(&cmd, &limits(), None).await?.log_tail)
}

/// The md5 of every decoded video frame of `path`, in order.
async fn frame_hashes(runner: &Runner, path: &Path) -> Result<Vec<String>> {
	let text = runner
		.capture(
			Tool::Ffmpeg,
			&[
				"-v",
				"error",
				"-i",
				&path.to_string_lossy(),
				"-map",
				"0:v:0",
				"-f",
				"framemd5",
				"-",
			],
			&limits(),
		)
		.await?;
	Ok(text
		.lines()
		.filter(|line| !line.starts_with('#') && !line.trim().is_empty())
		.filter_map(|line| line.rsplit(',').next().map(|md5| md5.trim().to_owned()))
		.collect())
}

/// The average PSNR of the first `frames` frames of `output` against the
/// source's frames from `from` on (seconds from the start of the file).
/// The source's frames are taken the way the cut takes them: a seek well
/// before, then an exact trim in the container's timestamps, since a seek
/// in a transport stream lands anywhere.
async fn head_psnr(
	runner: &Runner,
	source: &Path,
	from: f64,
	output: &Path,
	frames: u32,
) -> Result<f64> {
	let start_time = runner
		.probe(source, &limits())
		.await?
		.format
		.start_time
		.unwrap_or_default();
	let t = format!("{}", f64::from(frames) / FPS - 0.001);
	let reference = output.with_extension(format!("reference-{from}.nut"));
	let cmd = FfmpegCommand::new()
		.option("-ss", format!("{}", (from - 1.0).max(0.0)))
		.input(source)
		.flag("-copyts")
		.option("-ss", format!("{}", start_time + from - 0.001))
		.option("-t", &t)
		.option("-map", "0:v:0")
		.option("-c:v", "rawvideo")
		.option("-f", "nut")
		.output(&reference);
	runner.run(&cmd, &limits(), None).await?;
	let cmd = FfmpegCommand::new()
		.input(&reference)
		.option("-t", &t)
		.input(output)
		.option("-filter_complex", "[1:v][0:v]psnr")
		.option("-f", "null")
		.output("-");
	let log = runner.run(&cmd, &limits(), None).await?.log_tail;
	std::fs::remove_file(&reference)?;
	let line = log
		.lines()
		.find(|l| l.contains("average:"))
		.ok_or_else(|| format!("no PSNR in\n{log}"))?;
	let value = line
		.split("average:")
		.nth(1)
		.and_then(|rest| rest.split_whitespace().next())
		.ok_or_else(|| format!("bad PSNR line: {line}"))?;
	Ok(if value == "inf" {
		f64::INFINITY
	} else {
		value.parse()?
	})
}

/// When the beeps of `path` start, in the file's timestamps. (ffmpeg
/// reports them from the file's start time, which is added back; the
/// last entry is the end of the stream, where the final silence ends.)
async fn beeps(runner: &Runner, path: &Path) -> Result<Vec<f64>> {
	let start = runner
		.probe(path, &limits())
		.await?
		.format
		.start_time
		.unwrap_or_default();
	let cmd = FfmpegCommand::new()
		.input(path)
		.option("-map", "0:a:0")
		.option("-af", "silencedetect=n=-25dB:d=0.3")
		.option("-f", "null")
		.output("-");
	let log = runner.run(&cmd, &limits(), None).await?.log_tail;
	Ok(log
		.lines()
		.filter_map(|line| {
			let rest = line.split("silence_end:").nth(1)?;
			let at: f64 = rest.split_whitespace().next()?.parse().ok()?;
			Some(at + start)
		})
		.collect())
}

/// The output's video packets in decode order.
async fn packets(runner: &Runner, path: &Path) -> Result<Vec<Packet>> {
	let probe = runner.probe(path, &limits()).await?;
	let source = Source::from_probe(&probe, path)?;
	Ok(runner
		.packets_in(
			path,
			source.video_stream.unwrap_or_default(),
			&[(0.0, source.start_time + source.duration + 1.0)],
			&limits(),
		)
		.await?)
}

/// Where the clip's frames sit relative to its start: 0 for the MP4s,
/// the audio's lead for the transport stream, whose video starts after
/// its audio.
async fn grid_offset(runner: &Runner, clip: &Path) -> Result<f64> {
	let probe = runner.probe(clip, &limits()).await?;
	let source = Source::from_probe(&probe, clip)?;
	let first = runner
		.packets_in(
			clip,
			source.video_stream.unwrap_or_default(),
			&[(source.start_time, source.start_time + 1.0)],
			&limits(),
		)
		.await?
		.iter()
		.map(|p| p.pts)
		.fold(f64::INFINITY, f64::min);
	Ok((first - source.start_time).rem_euclid(1.0 / FPS))
}

/// The first frame at or after `time` on the clip's grid.
fn snap(time: f64, offset: f64) -> f64 {
	offset + ((time - 0.001 - offset) * FPS).ceil() / FPS
}

/// Cuts `clip` from 3.3 to 8.5 and checks the output frame by frame.
async fn check_head_body_tail(runner: &Runner, clip: &Path, dir: &Path, name: &str) -> Result<()> {
	let output = dir.join(format!("{name}.mkv"));
	let scratch = dir.join(format!(".{name}-scratch"));
	let request = request(&[(3.3, 8.5)], Export::SeparateFiles, &output);
	let plan = plan_on(runner, clip, &request, &scratch).await?;
	assert_eq!(plan.ranges.len(), 1);
	let range = &plan.ranges[0];
	// The cut lands on the first frame at or after 3.3 (3.32 on a grid
	// from 0) and ends before the first frame at or after 8.5 (8.52).
	let offset = grid_offset(runner, clip).await?;
	let start = snap(3.3, offset);
	let end = snap(8.5, offset);
	let (k1, k2) = (offset + 4.0, offset + 8.0);
	assert!((range.segment.start - start).abs() < 0.0005, "{range:?}");
	assert!((range.segment.end - end).abs() < 0.0005, "{range:?}");
	// Head to the keyframe at 4, body to the one at 8, tail to the end.
	let kinds: Vec<PieceKind> = range.pieces.iter().map(|p| p.kind).collect();
	assert!(
		matches!(
			kinds[..],
			[PieceKind::Encode, PieceKind::Copy { .. }, PieceKind::Encode]
		),
		"{kinds:?}"
	);
	assert!((range.pieces[0].end - k1).abs() < 0.0005, "{range:?}");
	assert!((range.pieces[1].end - k2).abs() < 0.0005, "{range:?}");
	assert!(
		(range.encoded_seconds() - (k1 - start + end - k2)).abs() < 0.0005,
		"{range:?}"
	);
	assert!(range.streams.is_some());
	let output = dir.join(format!("{name}-1.mkv"));
	assert_eq!(plan.outputs(), std::slice::from_ref(&output));
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "small counts"
	)]
	let (head, total) = (
		((k1 - start) * FPS).round() as usize,
		((end - start) * FPS).round() as usize,
	);
	assert_eq!(total, 130);

	export(runner, clip, &plan).await?;
	assert!(!scratch.exists());
	let errors = decode_errors(runner, &output).await?;
	assert!(
		errors.trim().is_empty(),
		"{name}: decoding reported:\n{errors}"
	);

	// 5.2 s at 25 fps: 130 frames, the body's keyframes where they were.
	let packets = packets(runner, &output).await?;
	assert_eq!(packets.len(), total, "{name}");
	let first = packets.iter().map(|p| p.pts).fold(f64::INFINITY, f64::min);
	let keyframes: Vec<f64> = packets
		.iter()
		.filter(|p| p.key)
		.map(|p| p.pts - first)
		.collect();
	assert_eq!(keyframes.len(), 6, "{name}: {keyframes:?}");
	let expected = std::iter::once(0.0).chain((0..5).map(|i| f64::from(i) + k1 - start));
	for (k, expected) in keyframes.iter().zip(expected) {
		assert!((k - expected).abs() < 0.0015, "{name}: {keyframes:?}");
	}

	// The body is the source's frames, bit for bit; the head and the tail
	// are its frames re-encoded, and not their neighbours.
	let out = frame_hashes(runner, &output).await?;
	let src = frame_hashes(runner, clip).await?;
	assert_eq!(out.len(), total, "{name}");
	assert_eq!(src.len(), 250, "{name}");
	assert_eq!(
		&out[head..head + 100],
		&src[100..200],
		"{name}: the body differs"
	);
	let frames = u32::try_from(head)?;
	let exact = head_psnr(runner, clip, start, &output, frames).await?;
	let off_by_one = head_psnr(runner, clip, start - 1.0 / FPS, &output, frames).await?;
	assert!(exact > 38.0, "{name}: head PSNR {exact} dB");
	assert!(
		off_by_one < exact - 5.0,
		"{name}: head PSNR {exact} dB against the right frames, {off_by_one} dB a frame early"
	);

	// The beeps at 4, 5, 6, 7 and 8 s of the source sit where the video of
	// those seconds sits: at the body's keyframes.
	let beeps = beeps(runner, &output).await?;
	let expected: Vec<f64> = keyframes[1..].iter().map(|k| k + first).collect();
	assert_eq!(
		beeps.len(),
		expected.len() + 1,
		"{name}: beeps at {beeps:?}, keyframes at {expected:?}"
	);
	for (beep, at) in beeps.iter().zip(&expected) {
		assert!(
			(beep - at).abs() < SYNC_TOLERANCE,
			"{name}: beep at {beep}, video at {at}"
		);
	}
	Ok(())
}

#[tokio::test]
async fn h264_cuts_are_frame_accurate_and_in_sync() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();
	check_head_body_tail(&runner, &clips.h264, dir.path(), "h264")
		.await
		.unwrap();
}

#[tokio::test]
async fn hevc_cuts_are_frame_accurate_and_in_sync() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();
	check_head_body_tail(&runner, &clips.hevc, dir.path(), "hevc")
		.await
		.unwrap();
}

#[tokio::test]
async fn transport_streams_are_cut_too() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();
	check_head_body_tail(&runner, &clips.h264_ts, dir.path(), "h264-ts")
		.await
		.unwrap();
}

#[tokio::test]
async fn a_merged_export_and_a_cut_from_the_start() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();

	// Two ranges into one file: 1.52 to 2.52 and 6.32 to 8.0.
	let output = dir.path().join("merged.mp4");
	let scratch = dir.path().join(".merged-scratch");
	let merged = request(&[(6.3, 8.0), (1.5, 2.5)], Export::Merged, &output);
	let plan = plan_on(&runner, &clips.h264, &merged, &scratch)
		.await
		.unwrap();
	assert_eq!(plan.ranges.len(), 2);
	assert_eq!(plan.outputs(), std::slice::from_ref(&output));
	// A range ending on a keyframe has no tail.
	assert_eq!(plan.ranges[1].pieces.len(), 2, "{:?}", plan.ranges[1]);
	export(&runner, &clips.h264, &plan).await.unwrap();
	let errors = decode_errors(&runner, &output).await.unwrap();
	assert!(errors.trim().is_empty(), "decoding reported:\n{errors}");
	let packets = packets(&runner, &output).await.unwrap();
	assert_eq!(packets.len(), 25 + 42, "{packets:?}");
	let out = frame_hashes(&runner, &output).await.unwrap();
	let src = frame_hashes(&runner, &clips.h264).await.unwrap();
	// The first range holds a single keyframe, so it is encoded whole; the
	// second range's body (7.0 to 8.0) is copied.
	assert_eq!(plan.ranges[0].pieces.len(), 1, "{:?}", plan.ranges[0]);
	assert_eq!(&out[25 + 17..67], &src[175..200]);
	let beeps = beeps(&runner, &output).await.unwrap();
	let first = packets.iter().map(|p| p.pts).fold(f64::INFINITY, f64::min);
	// The beep at 2 s of the source, then the one at 7 s: 0.48 s and
	// 1.0 + 0.68 s into the output.
	assert_eq!(beeps.len(), 3, "{beeps:?}");
	assert!(
		(beeps[0] - (first + 0.48)).abs() < SYNC_TOLERANCE,
		"{beeps:?}"
	);
	assert!(
		(beeps[1] - (first + 1.68)).abs() < SYNC_TOLERANCE,
		"{beeps:?}"
	);

	// From the very start: the first keyframe, whose decode time is
	// negative, is kept.
	let output = dir.path().join("start-{n}.mp4");
	let scratch = dir.path().join(".start-scratch");
	let from_start = request(&[(0.0, 1.5)], Export::SeparateFiles, &output);
	let plan = plan_on(&runner, &clips.h264, &from_start, &scratch)
		.await
		.unwrap();
	let range = &plan.ranges[0];
	let kinds: Vec<PieceKind> = range.pieces.iter().map(|p| p.kind).collect();
	assert!(
		matches!(kinds[..], [PieceKind::Copy { .. }, PieceKind::Encode]),
		"{range:?}"
	);
	export(&runner, &clips.h264, &plan).await.unwrap();
	let output = dir.path().join("start-1.mp4");
	let out = frame_hashes(&runner, &output).await.unwrap();
	assert_eq!(out.len(), 38);
	assert_eq!(&out[..25], &src[..25]);
}

#[tokio::test]
async fn unsupported_sources_are_named() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let audio = dir.path().join("tone.m4a");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("sine=frequency=440:duration=2")
		.option("-c:a", "aac")
		.output(&audio);
	runner.run(&cmd, &limits(), None).await.unwrap();
	let probe = runner.probe(&audio, &limits()).await.unwrap();
	let source = Source::from_probe(&probe, &audio).unwrap();
	assert_eq!(
		smart::check(&probe, &source, |_| true).unwrap_err(),
		smart::Unsupported::NoVideo
	);

	let vp9 = dir.path().join("vp9.webm");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=duration=1:size=64x64:rate=10")
		.option("-c:v", "libvpx-vp9")
		.option("-deadline", "realtime")
		.output(&vp9);
	runner.run(&cmd, &limits(), None).await.unwrap();
	let probe = runner.probe(&vp9, &limits()).await.unwrap();
	let source = Source::from_probe(&probe, &vp9).unwrap();
	let error = smart::check(&probe, &source, |_| true).unwrap_err();
	assert_eq!(
		error,
		smart::Unsupported::Codec {
			codec: "vp9".into()
		}
	);
	assert!(error.to_string().contains("H.264 and H.265"), "{error}");
}
