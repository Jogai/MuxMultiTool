//! The ffmpeg commands of a cut plan, each with a snapshot test. How the
//! options were chosen is in the module docs of [`super`].

use std::path::Path;

use super::{CutMode, DTS_SLACK, INPOINT_NUDGE, Plan, PlannedSegment, SEEK_MARGIN};
use crate::ffmpeg::FfmpegCommand;

/// One file per range: `input` is opened once per range with an input
/// seek just before its cut, then every output takes its own input with
/// `-copyts`, an exact output `-ss` and `-t`, and copies every stream.
#[must_use]
pub fn separate(plan: &Plan, input: &Path) -> FfmpegCommand {
	let mut cmd = FfmpegCommand::new();
	for segment in &plan.segments {
		let seek = plan.start_time + segment.cut_start - SEEK_MARGIN;
		if seek > 0.0 {
			cmd = cmd.option("-ss", secs(seek - plan.start_time));
		}
		cmd = cmd.input(input);
	}
	cmd = cmd.flag("-copyts");
	for (i, segment) in plan.segments.iter().enumerate() {
		let trim = trim_point(plan, segment);
		cmd = cmd
			.option("-map", i.to_string())
			.option("-ss", secs(trim))
			.option("-t", secs(plan.start_time + segment.segment.end - trim))
			.option("-c", "copy")
			.option("-avoid_negative_ts", "make_zero")
			.output(&segment.output);
	}
	cmd
}

/// Where a range's output `-ss` goes, in the container's timestamps: the
/// snapped keyframe's decode time, less a little, so the keyframe is kept;
/// the range's start otherwise.
fn trim_point(plan: &Plan, segment: &PlannedSegment) -> f64 {
	let relative = match (plan.cut_mode, segment.keyframe) {
		(CutMode::Keyframe, Some(keyframe)) => {
			keyframe.dts.unwrap_or(keyframe.pts - plan.reorder_delay) - DTS_SLACK
		}
		_ => segment.cut_start,
	};
	(plan.start_time + relative).max(0.0)
}

/// All ranges into one file through the concat demuxer, with the list as
/// a `data:` URL (see [`concat_list`]). The `file:` protocol must be
/// allowed explicitly, because a list that is not itself a file may only
/// open what its whitelist names.
#[must_use]
pub fn merged(plan: &Plan, input: &Path) -> FfmpegCommand {
	let output = plan
		.segments
		.first()
		.map(|s| s.output.clone())
		.unwrap_or_default();
	FfmpegCommand::new()
		.option("-f", "concat")
		.option("-safe", "0")
		.option("-protocol_whitelist", "data,file")
		.input(format!("data:text/plain,{}", concat_list(plan, input)))
		.option("-map", "0")
		.option("-c", "copy")
		.option("-avoid_negative_ts", "make_zero")
		.output(output)
}

/// The concat demuxer's list for a merged export: one entry per range
/// with its `inpoint` (the keyframe, nudged so the seek lands on it and
/// not on the one before),
/// `outpoint` and `duration` (so the next range follows without a gap or
/// an overlap).
#[must_use]
pub fn concat_list(plan: &Plan, input: &Path) -> String {
	use std::fmt::Write as _;
	let path = format!("file:{}", input.to_string_lossy()).replace('\'', r"'\''");
	let mut list = String::new();
	for segment in &plan.segments {
		// Writing to a String cannot fail.
		let _ = write!(
			list,
			"file '{path}'\ninpoint {}\noutpoint {}\nduration {}\n",
			secs(plan.start_time + segment.cut_start + INPOINT_NUDGE),
			secs(plan.start_time + segment.segment.end),
			secs(segment.length()),
		);
	}
	list
}

/// Seconds with microsecond precision and no trailing zeros, as ffmpeg
/// reads them: `3.05`, `12`.
fn secs(seconds: f64) -> String {
	let text = format!("{:.6}", seconds.max(0.0));
	let text = text.trim_end_matches('0').trim_end_matches('.');
	if text.is_empty() {
		"0".to_owned()
	} else {
		text.to_owned()
	}
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use super::*;
	use crate::cut::{Export, Segment};
	use crate::ffmpeg::probe::Keyframe;

	fn segment(
		number: usize,
		start: f64,
		end: f64,
		keyframe: Option<(f64, Option<f64>)>,
		output: &str,
	) -> PlannedSegment {
		let keyframe = keyframe.map(|(pts, dts)| Keyframe { pts, dts });
		PlannedSegment {
			number,
			segment: Segment::new(start, end),
			cut_start: keyframe.map_or(start, |k| k.pts),
			keyframe,
			output: PathBuf::from(output),
		}
	}

	fn input() -> PathBuf {
		PathBuf::from("/media/in/movie.mkv")
	}

	#[test]
	fn separate_files_keyframe_cut() {
		let plan = Plan {
			segments: vec![
				segment(
					1,
					0.4,
					65.25,
					Some((0.0, Some(-0.08))),
					"/media/out/movie-1.mkv",
				),
				segment(
					2,
					754.567,
					800.0,
					Some((752.0, Some(751.92))),
					"/media/out/movie-2.mkv",
				),
			],
			cut_mode: CutMode::Keyframe,
			export: Export::SeparateFiles,
			start_time: 0.0,
			reorder_delay: 0.08,
		};
		insta::assert_snapshot!(separate(&plan, &input()));
	}

	#[test]
	fn separate_files_normal_cut() {
		let plan = Plan {
			segments: vec![
				segment(1, 3.0, 5.5, None, "/media/out/a.mkv"),
				segment(2, 754.567, 800.0, None, "/media/out/b.mkv"),
			],
			cut_mode: CutMode::Normal,
			export: Export::SeparateFiles,
			start_time: 0.0,
			reorder_delay: 0.08,
		};
		insta::assert_snapshot!(separate(&plan, &input()));
	}

	#[test]
	fn separate_files_from_a_transport_stream_use_its_timestamps() {
		// A keyframe without a decode time takes the reorder delay.
		let plan = Plan {
			segments: vec![segment(
				1,
				10.0,
				20.0,
				Some((9.0, None)),
				"/media/out/movie-1.ts",
			)],
			cut_mode: CutMode::Keyframe,
			export: Export::SeparateFiles,
			start_time: 1.4,
			reorder_delay: 0.08,
		};
		insta::assert_snapshot!(separate(&plan, Path::new("/media/in/movie.ts")));
	}

	#[test]
	fn merged_keyframe_cut() {
		let plan = Plan {
			segments: vec![
				segment(
					1,
					3.0,
					5.5,
					Some((2.0, Some(1.92))),
					"/media/out/it's cut.mkv",
				),
				segment(
					2,
					754.567,
					800.0,
					Some((752.0, Some(751.92))),
					"/media/out/it's cut.mkv",
				),
			],
			cut_mode: CutMode::Keyframe,
			export: Export::Merged,
			start_time: 0.0,
			reorder_delay: 0.08,
		};
		insta::assert_snapshot!(concat_list(&plan, Path::new("/media/in/it's a movie.mkv")));
		insta::assert_snapshot!(merged(&plan, Path::new("/media/in/it's a movie.mkv")));
	}

	#[test]
	fn seconds_are_compact() {
		assert_eq!(secs(3.05), "3.05");
		assert_eq!(secs(12.0), "12");
		assert_eq!(secs(0.0), "0");
		assert_eq!(secs(-0.5), "0");
		assert_eq!(secs(754.567_000_1), "754.567");
		assert_eq!(secs(1.0 / 3.0), "0.333333");
	}
}
