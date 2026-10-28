//! The ffmpeg command of one preview run: segments from segment `n` on.
//!
//! One run produces consecutive MPEG-TS segments into its own folder, with
//! `-f segment`, and appends a line to a CSV list each time a segment is
//! complete. The session reads that list to know which files are safe to
//! serve. A seek outside the run's reach starts a new run; nothing in the
//! command depends on the previous one.
//!
//! Timestamps are kept absolute (`-copyts -start_at_zero`), so a segment
//! made by any run carries the time it has in the playlist and hls.js can
//! place segments from different runs on one timeline. The segment muxer's
//! split times are relative to the first video packet of the run, which is
//! exactly the segment start:
//! - a transcode seeks accurately to the segment start and forces a
//!   keyframe every segment length, counted from its first frame;
//! - a copy can only start at a keyframe, and the playlist boundaries are
//!   keyframes. ffmpeg's input seek subtracts a fixed margin (3/23 s) from
//!   the requested time before it looks for the preceding keyframe, so the
//!   run asks for the boundary plus that margin and lands on the boundary.
//!
//! `-readrate` keeps a run from racing ahead of the player: it converts at
//! most twice real time after an initial burst, which bounds CPU use and
//! the disk space of segments nobody asked for yet.

use std::path::Path;

use crate::ffmpeg::FfmpegCommand;

use super::plan::{MAX_WIDTH, Plan};
use super::playlist::Segment;

/// What ffmpeg's input seek takes off the requested time before it looks
/// for the keyframe to start at (`3 * AV_TIME_BASE / 23` in `ffmpeg_demux.c`).
pub const SEEK_MARGIN_SECS: f64 = 3.0 / 23.0;

/// Added to copy-run seek targets on top of [`SEEK_MARGIN_SECS`], so a
/// keyframe time rounded to microseconds still counts as "not after" it.
const SEEK_SLACK_SECS: f64 = 0.0005;

/// Tolerance of the split times, so a keyframe a few microseconds before a
/// boundary (rounding of the probed time) still starts the segment.
const SPLIT_DELTA_SECS: f64 = 0.001;

/// How much media a run converts at full speed before `-readrate` throttles it.
const INITIAL_BURST_SECS: u32 = 20;

/// The command for a run starting at segment `start`, writing `{n}.ts`
/// files into `dir` and the list of completed segments to `list`.
///
/// `start` must be a segment of the plan's playlist.
#[must_use]
pub fn run_command(
	source: &Path,
	plan: &Plan,
	start: usize,
	dir: &Path,
	list: &Path,
) -> FfmpegCommand {
	let playlist = &plan.playlist;
	let first = playlist.get(start).unwrap_or(Segment {
		start: 0.0,
		duration: 0.0,
	});
	let segment_secs = first.duration;

	let seek = if plan.video_copy && plan.video.is_some() {
		first.start + SEEK_MARGIN_SECS + SEEK_SLACK_SECS
	} else {
		first.start
	};
	let mut cmd = FfmpegCommand::new()
		.option("-readrate", "2")
		.option("-readrate_initial_burst", INITIAL_BURST_SECS.to_string())
		.option("-ss", secs(seek))
		.input(source)
		.flag("-copyts")
		.flag("-start_at_zero");

	if let Some(index) = plan.video {
		cmd = cmd.option("-map", format!("0:{index}"));
	}
	if let Some(index) = plan.audio {
		cmd = cmd.option("-map", format!("0:{index}"));
	}

	if plan.video.is_some() {
		cmd = if plan.video_copy {
			cmd.option("-c:v", "copy")
		} else {
			let mut cmd = cmd
				.option("-c:v", "libx264")
				.option("-preset", "veryfast")
				.option("-crf", "23")
				.option("-profile:v", "high")
				.option("-level", "4.1")
				.option("-pix_fmt", "yuv420p")
				// Only the forced keyframes, one per segment start.
				.option("-g", "1000")
				.option("-sc_threshold", "0")
				.option("-forced-idr", "1")
				.option(
					"-force_key_frames",
					format!("expr:gte(t,n_forced*{})", secs(segment_secs)),
				);
			if plan.downscale {
				cmd = cmd.option("-vf", format!("scale={MAX_WIDTH}:-2"));
			}
			cmd
		};
	}
	if plan.audio.is_some() {
		cmd = if plan.audio_copy {
			cmd.option("-c:a", "copy")
		} else {
			cmd.option("-c:a", "aac")
				.option("-b:a", "128k")
				.option("-ac", "2")
		};
	}

	cmd = cmd
		.option("-f", "segment")
		.option("-segment_format", "mpegts")
		.option("-segment_start_number", start.to_string())
		.option("-segment_time_delta", secs(SPLIT_DELTA_SECS))
		.option("-segment_list", list)
		.option("-segment_list_type", "csv")
		.option("-reset_timestamps", "0");
	let times: Vec<String> = playlist
		.segments()
		.get(start + 1..)
		.unwrap_or_default()
		.iter()
		.map(|s| secs(s.start - first.start))
		.collect();
	if !times.is_empty() {
		cmd = cmd.option("-segment_times", times.join(","));
	}
	cmd.output(dir.join("%d.ts"))
}

/// Seconds with microsecond precision, the resolution ffprobe reports.
fn secs(value: f64) -> String {
	format!("{value:.6}")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::preview::playlist::Playlist;

	fn transcode_plan() -> Plan {
		Plan {
			video: Some(0),
			audio: Some(2),
			video_copy: false,
			audio_copy: false,
			downscale: true,
			playlist: Playlist::grid(7.5, 2.0),
		}
	}

	fn remux_plan() -> Plan {
		Plan {
			video: Some(0),
			audio: Some(1),
			video_copy: true,
			audio_copy: true,
			downscale: false,
			playlist: Playlist::from_keyframes(&[0.0, 3.52, 7.0, 9.0], 12.0, 2.0, 30.0, 0.15)
				.unwrap(),
		}
	}

	#[test]
	fn transcode_run_from_the_second_segment() {
		let cmd = run_command(
			Path::new("/media/in/movie.mkv"),
			&transcode_plan(),
			1,
			Path::new("/tmp/mmt/s1/run-1"),
			Path::new("/tmp/mmt/s1/run-1/segments.csv"),
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn remux_run_from_a_keyframe_boundary() {
		let cmd = run_command(
			Path::new("/media/in/movie.mkv"),
			&remux_plan(),
			1,
			Path::new("/tmp/mmt/s2/run-1"),
			Path::new("/tmp/mmt/s2/run-1/segments.csv"),
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn audio_transcode_keeps_the_video() {
		let plan = Plan {
			audio_copy: false,
			..remux_plan()
		};
		let cmd = run_command(
			Path::new("in.mkv"),
			&plan,
			0,
			Path::new("out"),
			Path::new("out/segments.csv"),
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn last_segment_has_no_split_times() {
		let cmd = run_command(
			Path::new("in.mkv"),
			&transcode_plan(),
			3,
			Path::new("out"),
			Path::new("out/segments.csv"),
		);
		let text = cmd.to_string();
		assert!(!text.contains("-segment_times"), "{text}");
		assert!(text.contains("-ss 6.000000"), "{text}");
	}

	#[test]
	fn audio_only_source() {
		let plan = Plan {
			video: None,
			audio: Some(0),
			video_copy: true,
			audio_copy: false,
			downscale: false,
			playlist: Playlist::grid(5.0, 2.0),
		};
		let text = run_command(
			Path::new("in.flac"),
			&plan,
			0,
			Path::new("out"),
			Path::new("out/segments.csv"),
		)
		.to_string();
		assert!(!text.contains("-c:v"), "{text}");
		assert!(text.contains("-c:a aac"), "{text}");
		assert!(text.contains("-ss 0.000000"), "{text}");
	}
}
