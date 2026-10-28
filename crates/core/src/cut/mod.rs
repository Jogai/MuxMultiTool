//! Lossless cutting: segments, keyframe snapping and the export plan.
//!
//! The user marks [`Segment`]s of a file and keeps or removes them
//! ([`SegmentMode`]); the kept ranges are exported one file each or
//! joined into one ([`Export`]), copying every stream without re-encoding.
//! Since a video packet can only be decoded from a keyframe on, where a
//! range *starts* is the whole question; where it ends is free, within a
//! frame or two. [`CutMode`] decides:
//!
//! - **Keyframe cut:** the range starts at the last keyframe at or before
//!   its start time, for every stream. Nothing after the start is lost;
//!   up to one GOP is added in front. The output is the range plus that
//!   lead-in.
//! - **Normal cut:** the range starts at its start time: audio, subtitles
//!   and data begin exactly there, and the video joins at its next
//!   keyframe, so players show no picture for up to one GOP. Nothing before
//!   the start is included. The output is as long as the range.
//!
//! Smart cut, which re-encodes the few frames up to the next keyframe so a
//! normal cut has a picture from its first frame, is a later feature: it
//! builds on the same [`Plan`].
//!
//! # How ffmpeg is driven
//!
//! Measured on ffmpeg 8 (and consistent with its source):
//!
//! - An input `-ss` seeks the demuxer. Containers with a keyframe index
//!   (MP4, Matroska) land on a keyframe, but one keyframe *too early* when
//!   the target is exactly a keyframe's time, and ffmpeg itself moves the
//!   target back by 3/23 s for streams with B-frames. MPEG-TS has no index
//!   and lands wherever its binary search ends.
//! - An output `-ss` and `-t` drop packets by *decode* time, and, until
//!   the first keyframe passed, every non-keyframe. With `-copyts` their
//!   values are the container's own timestamps.
//!
//! So a separate-file cut does both ([`command::separate`]): an input seek
//! placed [`SEEK_MARGIN`] before the cut, which lands somewhere before it
//! whatever the container, then `-copyts` and an output `-ss` at the exact
//! cut: the keyframe's decode time for a keyframe cut (so the keyframe is
//! kept and the audio starts with it, within the reorder delay), the start
//! time itself for a normal cut. The output starts where the cut lands in
//! every container.
//!
//! A merged export ([`command::merged`]) feeds the ranges to the `concat`
//! demuxer as `inpoint`/`outpoint` pairs. Its seek is the demuxer's, so the
//! inpoint is nudged [`INPOINT_NUDGE`] past the keyframe, and a `duration`
//! per range keeps the joined timeline contiguous. Only containers that
//! seek by keyframe index can start a piece cleanly, hence
//! [`Source::seeks_by_index`], and only a video stream gives the seek a
//! keyframe to land on (audio-only Matroska, Ogg, MP3 and FLAC seek by
//! cluster, page or guess), hence merged exports need video. The concat
//! list travels inside the command as a `data:` URL, so a job needs no
//! scratch file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ffmpeg::probe::{CodecType, Keyframe, ProbeResult};

pub mod command;
pub mod project;
pub mod segments;
pub mod timecode;

pub use segments::{Segment, SegmentMode, coalesce_touching, invert, kept_segments, normalize};

/// How far before a cut the input seek is placed, in seconds, so the
/// demuxer lands before the cut whatever its seeking does.
pub const SEEK_MARGIN: f64 = 1.0;
/// How far past a keyframe a concat inpoint is placed, in seconds, so the
/// demuxer's seek lands on that keyframe rather than the one before.
pub const INPOINT_NUDGE: f64 = 0.05;
/// How close a time must be to a keyframe to count as that keyframe, in
/// seconds: a client that rounds to milliseconds still hits it.
pub const SNAP_TOLERANCE: f64 = 0.001;
/// How far before a keyframe's decode time its cut is placed, in seconds,
/// so rounding cannot drop the keyframe.
const DTS_SLACK: f64 = 0.001;

/// Where a range's start lands; see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CutMode {
	/// The last keyframe at or before the start.
	#[default]
	Keyframe,
	/// The start itself; the video joins at its next keyframe.
	Normal,
}

/// The shape of the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Export {
	/// One file per kept range.
	#[default]
	SeparateFiles,
	/// All kept ranges in one file.
	Merged,
}

/// What a cut needs to know about its source.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
	/// Length in seconds.
	pub duration: f64,
	/// The container's first timestamp in seconds: what the segments'
	/// zero corresponds to in the container's own timestamps.
	pub start_time: f64,
	/// The video stream the cuts are aligned to, or `None` for a file
	/// without a picture, where every packet is a keyframe.
	pub video_stream: Option<u32>,
	/// How long the video decoder holds frames back for reordering, in
	/// seconds: the gap between a keyframe's decode and presentation time
	/// when the container reports no decode time.
	pub reorder_delay: f64,
	/// ffprobe's `format_name`, e.g. `matroska,webm`.
	pub format_name: String,
	/// The input's file extension, for outputs that name none.
	pub extension: Option<String>,
}

/// Why a file cannot be cut.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
	/// ffprobe reports no length, so segments cannot be checked.
	#[error("the file has no known duration")]
	NoDuration,
}

impl Source {
	/// What `probe` says about the file at `input`.
	///
	/// # Errors
	/// [`SourceError::NoDuration`].
	pub fn from_probe(probe: &ProbeResult, input: &Path) -> Result<Self, SourceError> {
		let duration = probe
			.duration_secs()
			.filter(|d| d.is_finite() && *d > 0.0)
			.ok_or(SourceError::NoDuration)?;
		let video = probe
			.streams_of(CodecType::Video)
			.find(|s| !s.disposition.attached_pic);
		let reorder_delay = video
			.and_then(|v| {
				let frames = f64::from(v.has_b_frames?);
				let fps = v.frame_rate().filter(|f| f.is_finite() && *f > 0.0)?;
				Some(frames / fps)
			})
			.unwrap_or(0.0);
		Ok(Self {
			duration,
			start_time: probe
				.format
				.start_time
				.filter(|t| t.is_finite())
				.unwrap_or(0.0),
			video_stream: video.map(|v| v.index),
			reorder_delay,
			format_name: probe.format.format_name.clone(),
			extension: input.extension().map(|e| e.to_string_lossy().into_owned()),
		})
	}

	/// Whether the demuxer seeks by a keyframe index, which a merged
	/// export needs. MPEG transport and program streams carry none.
	#[must_use]
	pub fn seeks_by_index(&self) -> bool {
		!self
			.format_name
			.split(',')
			.any(|name| matches!(name, "mpegts" | "mpeg" | "mpegvideo"))
	}
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
	/// The segments, in any order.
	pub segments: Vec<Segment>,
	/// Whether the segments are kept or cut away.
	pub segment_mode: SegmentMode,
	/// See [`CutMode`].
	pub cut_mode: CutMode,
	/// See [`Export`].
	pub export: Export,
	/// The output file (merged), or a template for the files (separate):
	/// `{n}` is the range's number, zero-padded to the count, `{start}`
	/// and `{end}` its times as `HH.MM.SS.mmm`. A template without a
	/// placeholder gets `-{n}` before its extension. An output without an
	/// extension takes the input's.
	pub output: PathBuf,
}

/// One kept range as it will be cut.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedSegment {
	/// The range's number in the output, from 1.
	pub number: usize,
	/// The kept range, after normalisation.
	pub segment: Segment,
	/// Where the cut really begins: the keyframe's time when it snapped,
	/// the range's start otherwise. The output runs from here to the end.
	pub cut_start: f64,
	/// The keyframe the cut snapped to, with times relative to the start
	/// of the file.
	pub keyframe: Option<Keyframe>,
	/// The file this range goes to.
	pub output: PathBuf,
}

impl PlannedSegment {
	/// `end - cut_start`: how long the output of this range is.
	#[must_use]
	pub fn length(&self) -> f64 {
		self.segment.end - self.cut_start
	}
}

/// A request resolved against its source: what the job will do.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
	/// The kept ranges in output order.
	pub segments: Vec<PlannedSegment>,
	/// See [`CutMode`].
	pub cut_mode: CutMode,
	/// See [`Export`].
	pub export: Export,
	/// See [`Source::start_time`].
	pub start_time: f64,
	/// See [`Source::reorder_delay`].
	pub reorder_delay: f64,
}

/// Why a request cannot be planned.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlanError {
	/// After normalisation nothing is left to export.
	#[error("no range is left to export")]
	NoSegments,
	/// A keyframe cut found no keyframe at or before a range's start.
	/// The caller may retry with keyframes of the whole file.
	#[error("range {number} ({}) has no keyframe before its start", timecode::format(secs(*start)))]
	NoKeyframeBefore {
		/// The range's number.
		number: usize,
		/// Its start.
		start: f64,
	},
	/// A merged normal cut found no keyframe inside a range, so the range
	/// cannot start a piece. The caller may retry with keyframes of the
	/// whole file.
	#[error("range {number} ({} to {}) holds no keyframe, so a merged export cannot start there; export separate files or move its start", timecode::format(secs(*start)), timecode::format(secs(*end)))]
	NoKeyframeInside {
		/// The range's number.
		number: usize,
		/// Its start.
		start: f64,
		/// Its end.
		end: f64,
	},
	/// A merged export of a container without a keyframe index.
	#[error(
		"a merged export needs a container with a keyframe index, and {format} has none; export separate files"
	)]
	MergedNeedsIndex {
		/// ffprobe's format name.
		format: String,
	},
	/// A merged export of a file without video, whose demuxer cannot be
	/// relied on to seek to the exact packet.
	#[error("a merged export needs a video stream to align the pieces on; export separate files")]
	MergedNeedsVideo,
	/// Two ranges would be written to the same file.
	#[error("ranges {first} and {second} would both be written to {}; put {{n}} in the output name", path.display())]
	DuplicateOutput {
		/// The first range's number.
		first: usize,
		/// The second range's number.
		second: usize,
		/// The shared path.
		path: PathBuf,
	},
}

/// The intervals of the container's timestamps whose keyframes
/// [`plan`] needs for `request` on `source`, as
/// [`Runner::keyframes_in`](crate::ffmpeg::Runner::keyframes_in) takes
/// them. Empty when no keyframe is needed: a file without video, or a
/// normal cut into separate files, which ffmpeg aligns itself.
///
/// Each interval reaches `window` seconds around the range's start, so a
/// GOP longer than that may leave a range without a keyframe
/// ([`PlanError::NoKeyframeBefore`], [`PlanError::NoKeyframeInside`]);
/// retry with the whole file then.
#[must_use]
pub fn keyframe_windows(request: &Request, source: &Source, window: f64) -> Vec<(f64, f64)> {
	if source.video_stream.is_none() {
		return Vec::new();
	}
	let kept = kept_segments(&request.segments, request.segment_mode, source.duration);
	let snap = match (request.cut_mode, request.export) {
		(CutMode::Keyframe, _) => Snap::Back,
		(CutMode::Normal, Export::Merged) => Snap::Forward,
		(CutMode::Normal, Export::SeparateFiles) => return Vec::new(),
	};
	let end = source.start_time + source.duration;
	kept.iter()
		.map(|segment| {
			let start = source.start_time + segment.start;
			let (from, to) = match snap {
				Snap::Back => (start - window, start + SNAP_TOLERANCE),
				Snap::Forward => (
					start - SNAP_TOLERANCE,
					(start + window).min(segment.end + source.start_time),
				),
			};
			(
				from.max(source.start_time).max(0.0),
				to.min(end).max(from.max(0.0)),
			)
		})
		.collect()
}

/// The whole file as one interval, for the retry.
#[must_use]
pub fn whole_file_window(source: &Source) -> Vec<(f64, f64)> {
	vec![(
		source.start_time.max(0.0),
		source.start_time + source.duration,
	)]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Snap {
	Back,
	Forward,
}

/// Resolves `request` against `source`, snapping the kept ranges to
/// `keyframes` (in the container's timestamps, as
/// [`keyframe_windows`] asked for them).
///
/// # Errors
/// See [`PlanError`].
pub fn plan(request: &Request, source: &Source, keyframes: &[Keyframe]) -> Result<Plan, PlanError> {
	let mut kept = kept_segments(&request.segments, request.segment_mode, source.duration);
	if request.export == Export::Merged {
		if !source.seeks_by_index() {
			return Err(PlanError::MergedNeedsIndex {
				format: source.format_name.clone(),
			});
		}
		if source.video_stream.is_none() {
			return Err(PlanError::MergedNeedsVideo);
		}
		kept = coalesce_touching(&kept, SNAP_TOLERANCE);
	}
	if kept.is_empty() {
		return Err(PlanError::NoSegments);
	}
	let snap = match (source.video_stream, request.cut_mode, request.export) {
		(None, _, _) | (_, CutMode::Normal, Export::SeparateFiles) => None,
		(_, CutMode::Keyframe, _) => Some(Snap::Back),
		(_, CutMode::Normal, Export::Merged) => Some(Snap::Forward),
	};
	let relative: Vec<Keyframe> = keyframes
		.iter()
		.map(|k| Keyframe {
			pts: k.pts - source.start_time,
			dts: k.dts.map(|d| d - source.start_time),
		})
		.collect();
	let count = kept.len();
	let mut segments = Vec::with_capacity(count);
	for (i, segment) in kept.into_iter().enumerate() {
		let number = i + 1;
		let keyframe = match snap {
			None => None,
			Some(Snap::Back) => Some(
				relative
					.iter()
					.rev()
					.find(|k| k.pts <= segment.start + SNAP_TOLERANCE)
					.copied()
					.ok_or(PlanError::NoKeyframeBefore {
						number,
						start: segment.start,
					})?,
			),
			Some(Snap::Forward) => Some(
				relative
					.iter()
					.find(|k| k.pts >= segment.start - SNAP_TOLERANCE)
					.filter(|k| k.pts < segment.end)
					.copied()
					.ok_or(PlanError::NoKeyframeInside {
						number,
						start: segment.start,
						end: segment.end,
					})?,
			),
		};
		let cut_start = keyframe.map_or(segment.start, |k| k.pts.max(0.0));
		let output = output_path(
			&request.output,
			request.export,
			number,
			count,
			&segment,
			source.extension.as_deref(),
		);
		segments.push(PlannedSegment {
			number,
			segment,
			cut_start,
			keyframe,
			output,
		});
	}
	if request.export == Export::SeparateFiles {
		for (i, a) in segments.iter().enumerate() {
			if let Some(b) = segments[i + 1..].iter().find(|b| b.output == a.output) {
				return Err(PlanError::DuplicateOutput {
					first: a.number,
					second: b.number,
					path: a.output.clone(),
				});
			}
		}
	}
	Ok(Plan {
		segments,
		cut_mode: request.cut_mode,
		export: request.export,
		start_time: source.start_time,
		reorder_delay: source.reorder_delay,
	})
}

impl Plan {
	/// The files the job writes, in order, each once.
	#[must_use]
	pub fn outputs(&self) -> Vec<PathBuf> {
		let mut outputs: Vec<PathBuf> = Vec::new();
		for segment in &self.segments {
			if !outputs.contains(&segment.output) {
				outputs.push(segment.output.clone());
			}
		}
		outputs
	}

	/// The length of every output together, in seconds.
	#[must_use]
	pub fn total_length(&self) -> f64 {
		self.segments.iter().map(PlannedSegment::length).sum()
	}

	/// What ffmpeg's position reports count up to: the longest output for
	/// separate files (they are written side by side), the whole for a
	/// merged file.
	#[must_use]
	pub fn expected_duration(&self) -> Option<Duration> {
		let secs = match self.export {
			Export::SeparateFiles => self
				.segments
				.iter()
				.map(PlannedSegment::length)
				.fold(0.0, f64::max),
			Export::Merged => self.total_length(),
		};
		Duration::try_from_secs_f64(secs).ok()
	}

	/// The ffmpeg command that cuts `input` as planned.
	#[must_use]
	pub fn command(&self, input: &Path) -> crate::ffmpeg::FfmpegCommand {
		match self.export {
			Export::SeparateFiles => command::separate(self, input),
			Export::Merged => command::merged(self, input),
		}
	}

	/// The concat demuxer's list of a merged export; `None` for separate
	/// files.
	#[must_use]
	pub fn concat_list(&self, input: &Path) -> Option<String> {
		match self.export {
			Export::SeparateFiles => None,
			Export::Merged => Some(command::concat_list(self, input)),
		}
	}
}

/// The output of range `number` of `count`; see [`Request::output`].
fn output_path(
	template: &Path,
	export: Export,
	number: usize,
	count: usize,
	segment: &Segment,
	input_extension: Option<&str>,
) -> PathBuf {
	let has_extension = template.extension().is_some();
	let mut path = template.to_path_buf();
	if export == Export::SeparateFiles {
		let name = template
			.file_name()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_default();
		let name = if ["{n}", "{start}", "{end}"].iter().any(|p| name.contains(p)) {
			name
		} else {
			match name.rsplit_once('.') {
				Some((stem, ext)) if has_extension && !stem.is_empty() => {
					format!("{stem}-{{n}}.{ext}")
				}
				_ => format!("{name}-{{n}}"),
			}
		};
		let width = count.to_string().len();
		let name = name
			.replace("{n}", &format!("{number:0width$}"))
			.replace(
				"{start}",
				&timecode::format_for_filename(secs(segment.start)),
			)
			.replace("{end}", &timecode::format_for_filename(secs(segment.end)));
		path.set_file_name(name);
	}
	if !has_extension
		&& let Some(ext) = input_extension
		&& let Some(name) = path.file_name()
	{
		let name = format!("{}.{ext}", name.to_string_lossy());
		path.set_file_name(name);
	}
	path
}

/// Seconds as a duration; negative or absurd values become zero.
fn secs(seconds: f64) -> Duration {
	Duration::try_from_secs_f64(seconds).unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn source() -> Source {
		Source {
			duration: 100.0,
			start_time: 0.0,
			video_stream: Some(0),
			reorder_delay: 0.08,
			format_name: "matroska,webm".into(),
			extension: Some("mkv".into()),
		}
	}

	/// Keyframes every two seconds, with a two-frame reorder delay.
	fn keyframes() -> Vec<Keyframe> {
		(0..50)
			.map(|i| Keyframe {
				pts: f64::from(i) * 2.0,
				dts: Some(f64::from(i) * 2.0 - 0.08),
			})
			.collect()
	}

	fn request(cut_mode: CutMode, export: Export) -> Request {
		Request {
			segments: vec![Segment::new(10.5, 20.0), Segment::new(3.0, 5.25)],
			segment_mode: SegmentMode::Keep,
			cut_mode,
			export,
			output: PathBuf::from("/media/out/cut.mkv"),
		}
	}

	#[test]
	fn a_keyframe_cut_snaps_back_and_names_the_files() {
		let plan = plan(
			&request(CutMode::Keyframe, Export::SeparateFiles),
			&source(),
			&keyframes(),
		)
		.unwrap();
		assert_eq!(plan.segments.len(), 2);
		let [a, b] = &plan.segments[..] else { panic!() };
		assert_eq!(
			(a.number, a.segment, a.cut_start),
			(1, Segment::new(3.0, 5.25), 2.0)
		);
		assert_eq!(
			a.keyframe,
			Some(Keyframe {
				pts: 2.0,
				dts: Some(1.92)
			})
		);
		assert_eq!((b.number, b.cut_start), (2, 10.0));
		assert_eq!(a.output, PathBuf::from("/media/out/cut-1.mkv"));
		assert_eq!(b.output, PathBuf::from("/media/out/cut-2.mkv"));
		assert_eq!(plan.outputs(), [a.output.clone(), b.output.clone()]);
		assert!((plan.total_length() - 13.25).abs() < 1e-9);
		assert_eq!(
			plan.expected_duration(),
			Duration::try_from_secs_f64(10.0).ok()
		);
	}

	#[test]
	fn a_start_on_a_keyframe_stays_there() {
		let at = |start: f64| {
			let req = Request {
				segments: vec![Segment::new(start, 6.0)],
				..request(CutMode::Keyframe, Export::SeparateFiles)
			};
			plan(&req, &source(), &keyframes()).unwrap().segments[0].cut_start
		};
		// Within a millisecond of a keyframe, either way, is that keyframe.
		assert_eq!(at(4.0), 4.0);
		assert_eq!(at(4.0004), 4.0);
		assert_eq!(at(3.9996), 4.0);
		// Further before it is the previous one.
		assert_eq!(at(3.998), 2.0);
	}

	#[test]
	fn a_normal_cut_into_files_needs_no_keyframes() {
		let req = request(CutMode::Normal, Export::SeparateFiles);
		assert_eq!(
			keyframe_windows(&req, &source(), 30.0),
			[] as [(f64, f64); 0]
		);
		let plan = plan(&req, &source(), &[]).unwrap();
		assert_eq!(plan.segments[0].cut_start, 3.0);
		assert_eq!(plan.segments[0].keyframe, None);
		assert_eq!(
			plan.expected_duration(),
			Duration::try_from_secs_f64(9.5).ok()
		);
	}

	#[test]
	fn a_merged_normal_cut_snaps_forward() {
		let req = request(CutMode::Normal, Export::Merged);
		let plan = plan(&req, &source(), &keyframes()).unwrap();
		assert_eq!(plan.segments[0].cut_start, 4.0);
		assert_eq!(plan.segments[1].cut_start, 12.0);
		assert_eq!(plan.outputs(), [PathBuf::from("/media/out/cut.mkv")]);
		assert_eq!(
			plan.expected_duration(),
			Duration::try_from_secs_f64(9.25).ok()
		);
		// A range without a keyframe cannot start a piece.
		let short = Request {
			segments: vec![Segment::new(4.5, 5.5)],
			..req
		};
		assert_eq!(
			plan_err(&short),
			PlanError::NoKeyframeInside {
				number: 1,
				start: 4.5,
				end: 5.5
			}
		);
	}

	fn plan_err(req: &Request) -> PlanError {
		plan(req, &source(), &keyframes()).unwrap_err()
	}

	#[test]
	fn merged_exports_join_touching_ranges_and_need_an_index() {
		let req = Request {
			segments: vec![Segment::new(2.0, 4.0), Segment::new(4.0, 6.0)],
			..request(CutMode::Keyframe, Export::Merged)
		};
		let plan = plan(&req, &source(), &keyframes()).unwrap();
		assert_eq!(plan.segments.len(), 1);
		assert_eq!(plan.segments[0].segment, Segment::new(2.0, 6.0));
		let ts = Source {
			format_name: "mpegts".into(),
			..source()
		};
		assert!(!ts.seeks_by_index());
		assert_eq!(
			super::plan(&req, &ts, &keyframes()).unwrap_err(),
			PlanError::MergedNeedsIndex {
				format: "mpegts".into()
			}
		);
		// Separate files from a transport stream are fine.
		let files = Request {
			export: Export::SeparateFiles,
			..req
		};
		assert_eq!(
			super::plan(&files, &ts, &keyframes())
				.unwrap()
				.segments
				.len(),
			2
		);
	}

	#[test]
	fn remove_mode_keeps_the_gaps() {
		let req = Request {
			segment_mode: SegmentMode::Remove,
			..request(CutMode::Keyframe, Export::SeparateFiles)
		};
		let plan = plan(&req, &source(), &keyframes()).unwrap();
		let kept: Vec<Segment> = plan.segments.iter().map(|s| s.segment).collect();
		assert_eq!(
			kept,
			[
				Segment::new(0.0, 3.0),
				Segment::new(5.25, 10.5),
				Segment::new(20.0, 100.0)
			]
		);
		assert_eq!(plan.segments[1].cut_start, 4.0);
		assert_eq!(
			plan.segments[2].output,
			PathBuf::from("/media/out/cut-3.mkv")
		);
	}

	#[test]
	fn empty_and_missing_keyframes_are_errors() {
		let empty = Request {
			segments: vec![Segment::new(5.0, 5.0), Segment::new(200.0, 300.0)],
			..request(CutMode::Keyframe, Export::SeparateFiles)
		};
		assert_eq!(plan_err(&empty), PlanError::NoSegments);
		assert_eq!(
			plan(
				&request(CutMode::Keyframe, Export::SeparateFiles),
				&source(),
				&[]
			)
			.unwrap_err(),
			PlanError::NoKeyframeBefore {
				number: 1,
				start: 3.0
			}
		);
		assert_eq!(
			PlanError::NoKeyframeBefore {
				number: 1,
				start: 3.0
			}
			.to_string(),
			"range 1 (0:00:03.000) has no keyframe before its start"
		);
	}

	#[test]
	fn a_source_without_video_never_snaps() {
		let audio = Source {
			video_stream: None,
			..source()
		};
		let req = request(CutMode::Keyframe, Export::SeparateFiles);
		assert_eq!(keyframe_windows(&req, &audio, 30.0), [] as [(f64, f64); 0]);
		let plan = plan(&req, &audio, &[]).unwrap();
		assert_eq!(plan.segments[0].cut_start, 3.0);
		assert_eq!(plan.segments[0].keyframe, None);
		// Its pieces cannot be joined, though.
		let merged = request(CutMode::Keyframe, Export::Merged);
		assert_eq!(
			super::plan(&merged, &audio, &[]).unwrap_err(),
			PlanError::MergedNeedsVideo
		);
	}

	#[test]
	fn windows_surround_the_starts_in_container_time() {
		let ts = Source {
			start_time: 1.4,
			..source()
		};
		let req = request(CutMode::Keyframe, Export::SeparateFiles);
		let windows = keyframe_windows(&req, &ts, 30.0);
		assert_eq!(windows.len(), 2);
		assert!((windows[0].0 - 1.4).abs() < 1e-9, "{windows:?}");
		assert!((windows[0].1 - 4.401).abs() < 1e-9, "{windows:?}");
		assert!(
			(windows[1].0 - (11.9 - 30.0f64).max(1.4)).abs() < 1e-9,
			"{windows:?}"
		);
		let forward = keyframe_windows(
			&Request {
				cut_mode: CutMode::Normal,
				export: Export::Merged,
				..req
			},
			&ts,
			30.0,
		);
		assert!((forward[0].0 - 4.399).abs() < 1e-9, "{forward:?}");
		assert!((forward[0].1 - 6.65).abs() < 1e-9, "{forward:?}");
		assert_eq!(whole_file_window(&ts), [(1.4, 101.4)]);
	}

	#[test]
	fn keyframes_are_read_relative_to_the_container_start() {
		let ts = Source {
			start_time: 1.4,
			..source()
		};
		let shifted: Vec<Keyframe> = keyframes()
			.iter()
			.map(|k| Keyframe {
				pts: k.pts + 1.4,
				dts: k.dts.map(|d| d + 1.4),
			})
			.collect();
		let plan = plan(
			&request(CutMode::Keyframe, Export::SeparateFiles),
			&ts,
			&shifted,
		)
		.unwrap();
		assert!((plan.segments[0].cut_start - 2.0).abs() < 1e-9);
		assert!((plan.segments[0].keyframe.unwrap().dts.unwrap() - 1.92).abs() < 1e-9);
	}

	#[test]
	fn output_names_follow_the_template() {
		let seg = Segment::new(754.567, 800.0);
		let name = |template: &str, export| {
			output_path(Path::new(template), export, 3, 12, &seg, Some("mkv"))
		};
		assert_eq!(
			name("/out/x.mp4", Export::SeparateFiles),
			PathBuf::from("/out/x-03.mp4")
		);
		assert_eq!(
			name("/out/x", Export::SeparateFiles),
			PathBuf::from("/out/x-03.mkv")
		);
		assert_eq!(
			name("/out/part{n}.mkv", Export::SeparateFiles),
			PathBuf::from("/out/part03.mkv")
		);
		assert_eq!(
			name("/out/x {start}-{end}", Export::SeparateFiles),
			PathBuf::from("/out/x 00.12.34.567-00.13.20.000.mkv")
		);
		assert_eq!(
			name("/out/x.mp4", Export::Merged),
			PathBuf::from("/out/x.mp4")
		);
		assert_eq!(name("/out/x", Export::Merged), PathBuf::from("/out/x.mkv"));
		assert_eq!(
			name("/out/.hidden", Export::SeparateFiles),
			PathBuf::from("/out/.hidden-03.mkv")
		);
		assert_eq!(
			output_path(Path::new("/out/x"), Export::Merged, 1, 1, &seg, None),
			PathBuf::from("/out/x")
		);
	}

	#[test]
	fn time_templates_name_each_range() {
		let req = Request {
			segments: vec![Segment::new(1.0, 2.0), Segment::new(3.0, 4.0)],
			output: PathBuf::from("/out/{start}.mkv"),
			..request(CutMode::Normal, Export::SeparateFiles)
		};
		let plan = plan(&req, &source(), &[]).unwrap();
		assert_eq!(
			plan.outputs(),
			[
				PathBuf::from("/out/00.00.01.000.mkv"),
				PathBuf::from("/out/00.00.03.000.mkv")
			]
		);
		assert_eq!(
			PlanError::DuplicateOutput {
				first: 1,
				second: 2,
				path: PathBuf::from("/out/x.mkv")
			}
			.to_string(),
			"ranges 1 and 2 would both be written to /out/x.mkv; put {n} in the output name"
		);
	}

	#[test]
	fn the_source_comes_from_the_probe() {
		let probe = crate::ffmpeg::probe::parse(
			r#"{"format":{"format_name":"mpegts","start_time":"1.456778","duration":"10.02"},"streams":[
				{"index":0,"codec_type":"video","disposition":{"attached_pic":1}},
				{"index":1,"codec_type":"audio"},
				{"index":2,"codec_type":"video","has_b_frames":2,"avg_frame_rate":"25/1"}
			]}"#,
		)
		.unwrap();
		let source = Source::from_probe(&probe, Path::new("/media/in/clip.ts")).unwrap();
		assert_eq!(source.video_stream, Some(2));
		assert!((source.reorder_delay - 0.08).abs() < 1e-9);
		assert!((source.start_time - 1.456_778).abs() < 1e-9);
		assert_eq!(source.extension.as_deref(), Some("ts"));
		assert!(!source.seeks_by_index());

		let audio = crate::ffmpeg::probe::parse(
			r#"{"format":{"format_name":"flac","duration":"3"},"streams":[{"index":0,"codec_type":"audio"}]}"#,
		)
		.unwrap();
		let source = Source::from_probe(&audio, Path::new("x.flac")).unwrap();
		assert_eq!(
			(source.video_stream, source.reorder_delay, source.start_time),
			(None, 0.0, 0.0)
		);
		assert!(source.seeks_by_index());

		let none = crate::ffmpeg::probe::parse(r#"{"format":{},"streams":[]}"#).unwrap();
		assert_eq!(
			Source::from_probe(&none, Path::new("x")).unwrap_err(),
			SourceError::NoDuration
		);
	}
}
