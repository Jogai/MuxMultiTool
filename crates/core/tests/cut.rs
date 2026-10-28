//! Lossless cuts with the real ffmpeg and ffprobe from `PATH`: keyframe
//! lookup, keyframe and normal cuts into separate files and one merged
//! file, keep and remove modes, MP4, Matroska, MPEG-TS and an audio-only
//! source. Every output's length is checked against the plan: within one
//! GOP for a keyframe cut, within a few frames for a normal cut.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::cut::{self, CutMode, Export, Plan, Request, Segment, SegmentMode, Source};
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, ProbeResult, Runner};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const TIMEOUT: Duration = Duration::from_secs(120);
/// The clips are ten seconds with a keyframe every second.
const CLIP_SECS: f64 = 10.0;
const GOP: f64 = 1.0;
/// Six frames at 25 fps: the reorder delay at both ends of a cut, plus a
/// frame or two for the dts-based end. Far below a GOP.
const FRAMES: f64 = 0.25;

fn limits() -> Limits {
	Limits::timeout(TIMEOUT)
}

/// Ten seconds of test pattern and tone, H.264 with three B-frames and a
/// keyframe every second, plus AAC, as MP4; then remuxed to Matroska and
/// MPEG-TS; plus an M4A with the tone alone.
struct Clips {
	mp4: PathBuf,
	mkv: PathBuf,
	ts: PathBuf,
	audio: PathBuf,
}

async fn make_clips(runner: &Runner, dir: &Path) -> Result<Clips> {
	let mp4 = dir.join("clip.mp4");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=duration=10:size=320x240:rate=25")
		.option("-f", "lavfi")
		.input("sine=frequency=440:duration=10")
		.option("-c:v", "libx264")
		.option("-preset", "veryfast")
		.option("-bf", "3")
		.option("-g", "25")
		.option("-keyint_min", "25")
		.option("-sc_threshold", "0")
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
		.output(&mp4);
	runner.run(&cmd, &limits(), None).await?;
	let mkv = dir.join("clip.mkv");
	let ts = dir.join("clip.ts");
	for remux in [&mkv, &ts] {
		let cmd = FfmpegCommand::new()
			.input(&mp4)
			.option("-c", "copy")
			.output(remux);
		runner.run(&cmd, &limits(), None).await?;
	}
	let audio = dir.join("tone.m4a");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("sine=frequency=440:duration=10")
		.option("-c:a", "aac")
		.output(&audio);
	runner.run(&cmd, &limits(), None).await?;
	Ok(Clips {
		mp4,
		mkv,
		ts,
		audio,
	})
}

async fn probe(runner: &Runner, path: &Path) -> Result<ProbeResult> {
	Ok(runner.probe(path, &limits()).await?)
}

async fn source(runner: &Runner, path: &Path) -> Result<Source> {
	Ok(Source::from_probe(&probe(runner, path).await?, path)?)
}

/// Plans `request` on `input` the way the server does: keyframes around
/// the cuts, the whole file when that is not enough.
async fn plan_on(runner: &Runner, input: &Path, request: &Request) -> Result<Plan> {
	let source = source(runner, input).await?;
	let windows = cut::keyframe_windows(request, &source, 5.0);
	let stream = source.video_stream.unwrap_or_default();
	let keyframes = if windows.is_empty() {
		Vec::new()
	} else {
		runner
			.keyframes_in(input, stream, &windows, &limits())
			.await?
	};
	Ok(cut::plan(request, &source, &keyframes)?)
}

/// Runs the plan and returns the probe of each output, in order.
async fn export(runner: &Runner, input: &Path, plan: &Plan) -> Result<Vec<ProbeResult>> {
	runner.run(&plan.command(input), &limits(), None).await?;
	let mut probes = Vec::new();
	for output in plan.outputs() {
		assert!(output.is_file(), "{} was not written", output.display());
		probes.push(probe(runner, &output).await?);
	}
	Ok(probes)
}

/// The length of the first stream of `kind`, from ffprobe.
fn stream_length(probe: &ProbeResult, kind: CodecType) -> f64 {
	probe
		.streams_of(kind)
		.next()
		.and_then(|s| s.duration)
		.or(probe.format.duration)
		.unwrap_or(f64::NAN)
}

fn cut_request(
	segments: &[(f64, f64)],
	cut_mode: CutMode,
	export: Export,
	output: &Path,
) -> Request {
	Request {
		segments: segments.iter().map(|(s, e)| Segment::new(*s, *e)).collect(),
		segment_mode: SegmentMode::Keep,
		cut_mode,
		export,
		output: output.to_path_buf(),
	}
}

#[tokio::test]
async fn keyframes_are_listed_for_a_range() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();
	for clip in [&clips.mp4, &clips.mkv, &clips.ts] {
		let source = source(&runner, clip).await.unwrap();
		let stream = source.video_stream.unwrap();
		let from = source.start_time + 2.5;
		let to = source.start_time + 6.5;
		let keyframes = runner
			.keyframes_in(clip, stream, &[(from, to)], &limits())
			.await
			.unwrap();
		// Keyframes at 3, 4, 5 and 6 s; the transport stream's video starts
		// a few milliseconds after its audio, so its times are relative to
		// that.
		let times: Vec<f64> = keyframes
			.iter()
			.map(|k| k.pts - source.start_time)
			.filter(|t| *t >= 2.5)
			.collect();
		assert_eq!(times.len(), 4, "{}: {keyframes:?}", clip.display());
		assert!(
			(3.0..3.1).contains(&times[0]),
			"{}: {times:?}",
			clip.display()
		);
		for pair in times.windows(2) {
			assert!(
				(pair[1] - pair[0] - 1.0).abs() < 0.001,
				"{}: {times:?}",
				clip.display()
			);
		}
		// The decode time, where reported (Matroska leaves it out on the
		// keyframe the seek lands on), precedes the presentation time by the
		// reorder delay.
		let with_dts = keyframes.iter().filter(|k| k.dts.is_some()).count();
		assert!(with_dts >= 4, "{}: {keyframes:?}", clip.display());
		for k in &keyframes {
			if let Some(dts) = k.dts {
				assert!(
					(k.pts - dts - source.reorder_delay).abs() < 0.001,
					"{}: {k:?} delay {}",
					clip.display(),
					source.reorder_delay
				);
			}
		}
		assert!(source.reorder_delay > 0.0, "{}", clip.display());
	}
	// An audio-only file has no keyframes to speak of.
	let audio = source(&runner, &clips.audio).await.unwrap();
	assert_eq!(audio.video_stream, None);
	assert_eq!(audio.duration.round(), CLIP_SECS);
}

#[tokio::test]
async fn keyframe_cuts_start_on_the_keyframe_in_every_container() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();
	// The transport stream's pieces go to Matroska: ffprobe only estimates
	// the stream lengths of an MPEG-TS file, and the check below needs them.
	for (clip, ext, out) in [
		(&clips.mp4, "mp4", "mp4"),
		(&clips.mkv, "mkv", "mkv"),
		(&clips.ts, "ts", "mkv"),
	] {
		let output = dir.path().join(format!("kf-{ext}-{{n}}.{out}"));
		let request = cut_request(
			&[(6.3, 8.2), (3.3, 5.5)],
			CutMode::Keyframe,
			Export::SeparateFiles,
			&output,
		);
		let plan = plan_on(&runner, clip, &request).await.unwrap();
		let starts: Vec<f64> = plan.segments.iter().map(|s| s.cut_start.round()).collect();
		assert_eq!(starts, [3.0, 6.0], "{ext}: {plan:?}");
		let probes = export(&runner, clip, &plan).await.unwrap();
		for (segment, probe) in plan.segments.iter().zip(&probes) {
			let requested = segment.segment.length();
			let planned = segment.length();
			let video = stream_length(probe, CodecType::Video);
			let audio = stream_length(probe, CodecType::Audio);
			// The output is the range plus the lead-in to the keyframe: never
			// shorter than asked, never a GOP longer.
			assert!(
				video >= requested - FRAMES && video < requested + GOP,
				"{ext} range {}: video {video} s for {requested} s requested",
				segment.number
			);
			assert!(
				(video - planned).abs() < FRAMES,
				"{ext} range {}: video {video} s, planned {planned} s",
				segment.number
			);
			assert!(
				(audio - planned).abs() < FRAMES,
				"{ext} range {}: audio {audio} s, planned {planned} s",
				segment.number
			);
			let first = probe.streams_of(CodecType::Video).next().unwrap();
			assert_eq!(first.codec_name.as_deref(), Some("h264"));
		}
	}
}

#[tokio::test]
async fn normal_cuts_start_on_the_second_and_join_the_video_later() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();
	let output = dir.path().join("normal-{n}.mkv");
	let request = cut_request(
		&[(3.3, 5.5), (6.3, 8.2)],
		CutMode::Normal,
		Export::SeparateFiles,
		&output,
	);
	let plan = plan_on(&runner, &clips.mkv, &request).await.unwrap();
	assert!(plan.segments.iter().all(|s| s.keyframe.is_none()));
	let probes = export(&runner, &clips.mkv, &plan).await.unwrap();
	for (segment, probe) in plan.segments.iter().zip(&probes) {
		let requested = segment.segment.length();
		let total = probe.format.duration.unwrap_or(f64::NAN);
		let video_start = probe
			.streams_of(CodecType::Video)
			.next()
			.and_then(|s| s.start_time)
			.unwrap_or(f64::NAN);
		// The file is as long as the range: the audio starts on the second.
		assert!(
			(total - requested).abs() < FRAMES,
			"range {}: {total} s for {requested} s requested",
			segment.number
		);
		// The video joins at the next keyframe: 4.0 for a start at 3.3, 7.0
		// for one at 6.3.
		let join = segment.segment.start.ceil() - segment.segment.start;
		assert!(
			(video_start - join).abs() < FRAMES,
			"range {}: video starts at {video_start} s, expected {join} s",
			segment.number
		);
	}
}

#[tokio::test]
async fn merged_exports_join_the_ranges() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();
	for (mode, expected) in [(CutMode::Keyframe, 2.5 + 2.2), (CutMode::Normal, 1.5 + 1.2)] {
		let output = dir.path().join(format!("merged-{mode:?}.mp4"));
		let request = cut_request(&[(3.3, 5.5), (6.3, 8.2)], mode, Export::Merged, &output);
		let plan = plan_on(&runner, &clips.mp4, &request).await.unwrap();
		assert_eq!(plan.outputs(), std::slice::from_ref(&output));
		let list = plan.concat_list(&clips.mp4).unwrap();
		assert_eq!(list.matches("inpoint").count(), 2, "{list}");
		let probes = export(&runner, &clips.mp4, &plan).await.unwrap();
		let total = stream_length(&probes[0], CodecType::Video);
		assert!(
			(total - expected).abs() < 2.0 * FRAMES + 0.1,
			"{mode:?}: {total} s for {expected} s planned"
		);
		assert!((plan.total_length() - expected).abs() < 0.001);
	}
	// A transport stream cannot be merged, but its pieces can be cut.
	let request = cut_request(
		&[(3.3, 5.5)],
		CutMode::Keyframe,
		Export::Merged,
		&dir.path().join("merged.ts"),
	);
	let error = plan_on(&runner, &clips.ts, &request)
		.await
		.unwrap_err()
		.to_string();
	assert!(error.contains("keyframe index"), "{error}");
}

#[tokio::test]
async fn remove_mode_keeps_the_rest() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();
	let request = Request {
		segment_mode: SegmentMode::Remove,
		..cut_request(
			&[(3.3, 5.5)],
			CutMode::Keyframe,
			Export::SeparateFiles,
			&dir.path().join("rest"),
		)
	};
	let plan = plan_on(&runner, &clips.mkv, &request).await.unwrap();
	let kept: Vec<(f64, f64)> = plan
		.segments
		.iter()
		.map(|s| (s.cut_start.round(), s.segment.end.round()))
		.collect();
	assert_eq!(kept, [(0.0, 3.0), (5.0, 10.0)], "{plan:?}");
	assert_eq!(
		plan.outputs(),
		[dir.path().join("rest-1.mkv"), dir.path().join("rest-2.mkv")]
	);
	let probes = export(&runner, &clips.mkv, &plan).await.unwrap();
	let lengths: Vec<f64> = probes
		.iter()
		.map(|p| stream_length(p, CodecType::Video))
		.collect();
	assert!((lengths[0] - 3.3).abs() < FRAMES, "{lengths:?}");
	assert!((lengths[1] - 5.0).abs() < FRAMES, "{lengths:?}");
}

#[tokio::test]
async fn audio_only_sources_cut_exactly() {
	let runner = Runner::default();
	let dir = tempfile::tempdir().unwrap();
	let clips = make_clips(&runner, dir.path()).await.unwrap();
	for mode in [CutMode::Keyframe, CutMode::Normal] {
		let request = cut_request(
			&[(1.0, 2.0), (3.0, 4.5)],
			mode,
			Export::SeparateFiles,
			&dir.path().join(format!("tone-{mode:?}-{{n}}")),
		);
		let plan = plan_on(&runner, &clips.audio, &request).await.unwrap();
		assert!(plan.segments.iter().all(|s| s.keyframe.is_none()));
		assert_eq!(
			plan.outputs(),
			[
				dir.path().join(format!("tone-{mode:?}-1.m4a")),
				dir.path().join(format!("tone-{mode:?}-2.m4a"))
			]
		);
		let probes = export(&runner, &clips.audio, &plan).await.unwrap();
		let lengths: Vec<f64> = probes
			.iter()
			.map(|p| stream_length(p, CodecType::Audio))
			.collect();
		assert!((lengths[0] - 1.0).abs() < 0.1, "{mode:?}: {lengths:?}");
		assert!((lengths[1] - 1.5).abs() < 0.1, "{mode:?}: {lengths:?}");
	}
	// Audio-only pieces are not joined: the demuxers seek too coarsely.
	let request = cut_request(
		&[(1.0, 2.0), (3.0, 4.5)],
		CutMode::Keyframe,
		Export::Merged,
		&dir.path().join("tone-cut.m4a"),
	);
	let error = plan_on(&runner, &clips.audio, &request)
		.await
		.unwrap_err()
		.to_string();
	assert!(error.contains("video stream"), "{error}");
}
