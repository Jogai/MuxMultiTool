//! Frame-accurate smart cut: re-encode only around the cut points.
//!
//! A lossless cut can only start a range at a keyframe, so a keyframe cut
//! starts up to a GOP early and a normal cut shows no picture for up to a
//! GOP. A smart cut starts and ends on the chosen frames and keeps almost
//! everything lossless: of each kept range, the frames from its start up
//! to the first keyframe (the *head*) and from the last keyframe's GOP
//! boundary to its end (the *tail*) are decoded and encoded again with the
//! source's codec; the frames between the two keyframes (the *body*) are
//! copied. Audio, subtitles and attachments are always copied. The parts
//! are then joined.
//!
//! ```text
//!   source   ‥‥K‥‥‥‥‥‥‥K‥‥‥‥‥‥‥K‥‥‥‥‥‥‥K‥‥‥‥‥‥‥K‥‥
//!   range          [start ─────────────────── end)
//!   pieces          head │     body (copied)     │tail
//!                   K1 ──┘                       └── K2
//! ```
//!
//! # How it runs
//!
//! Two ffmpeg commands ([`Plan::commands`]). The first ([`Plan::prepare`])
//! writes the *pieces* to a scratch folder: one MPEG-TS file per video
//! part, encoded with x264/x265 or copied, and one Matroska file per range
//! with the other streams, copied. The second ([`Plan::assemble`]) joins
//! them with the concat demuxer into the output file(s).
//!
//! - The video pieces are MPEG-TS because that container carries the
//!   codec's parameter sets *in the stream*, before every keyframe
//!   (x264/x265 repeat their headers for it, and a copied stream gets them
//!   from `h264_mp4toannexb`/`hevc_mp4toannexb`). An encoded part has
//!   different parameters than the source, and a decoder can only follow
//!   the change when each part announces its own. The MP4 and Matroska
//!   muxers keep those in-stream parameter sets.
//! - A piece's first frame is exactly its nominal start (a cut snapped
//!   to a frame, or a keyframe), so the concat demuxer, which places each
//!   file at the sum of the `duration`s before it and shifts its
//!   timestamps by its own start, puts every frame at its source time
//!   minus the range's start. The copied streams go through a second
//!   concat input with an `inpoint 0`, which makes the demuxer take their
//!   timestamps as they are (the piece starts at the range's start). The
//!   streams of a range therefore keep their relative timing exactly; the
//!   join keeps them with `-copyts` (ffmpeg would otherwise move each
//!   input to start at zero, and the copied streams begin a packet after
//!   the range's start), and `-avoid_negative_ts make_zero` shifts them
//!   all alike.
//! - Encoding trims by presentation time (ffmpeg's `trim` filter), so the
//!   head holds the frames from the cut up to the keyframe and the tail
//!   the frames from the GOP boundary to the end. Copying trims by decode
//!   time ([`super`]), so the body is cut at the keyframes' decode times:
//!   everything decoded from K1 on and before K2.
//! - A cut point lands on the first frame at or after it, read from the
//!   packets around the cut ([`windows`]). A cut that is not on a frame is
//!   moved to the next one; a range ends *before* the frame at its end
//!   time, so `[2, 3)` at 25 fps is exactly 25 frames.
//!
//! # What is not supported
//!
//! The encoders are x264 and x265 of the GPL build, so the video must be
//! H.264 or H.265 ([`Unsupported::Codec`]). Interlaced video is refused,
//! since the pieces would be encoded progressive. Open GOPs (frames after
//! a keyframe in decode order that are shown before it) are refused: such
//! frames would reference the re-encoded head, which they cannot decode
//! from. See [`check`] and [`Unsupported`].

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::{Export, PlannedSegment, SEEK_MARGIN, SNAP_TOLERANCE, Segment, Source};
use crate::ffmpeg::FfmpegCommand;
use crate::ffmpeg::probe::{CodecType, Packet, ProbeResult, Stream};

/// The encoded parts' quality: x264's and x265's constant rate factor,
/// near-transparent for the few frames involved.
pub const CRF_H264: u32 = 18;
/// See [`CRF_H264`].
pub const CRF_HEVC: u32 = 20;
/// The encoders' speed/quality preset. The parts are short, so speed is
/// not the concern.
pub const PRESET: &str = "medium";
/// How far before a keyframe's decode time the body copy starts, in
/// seconds, so rounding cannot drop the keyframe; and how far before the
/// next keyframe's decode time it ends, so rounding cannot keep it.
const DTS_SLACK: f64 = super::DTS_SLACK;
/// How far past a range's end the packets are read, in seconds, so the
/// frame at the end (where the range stops) is seen.
const END_MARGIN: f64 = 0.5;

/// The video codecs a smart cut can re-encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
	/// H.264 / AVC, encoded with x264.
	H264,
	/// H.265 / HEVC, encoded with x265.
	Hevc,
}

impl Codec {
	/// The codec of ffprobe's `codec_name`, when it is one of the two.
	#[must_use]
	pub fn from_name(name: &str) -> Option<Self> {
		match name {
			"h264" => Some(Self::H264),
			"hevc" => Some(Self::Hevc),
			_ => None,
		}
	}

	/// The ffmpeg encoder that writes this codec.
	#[must_use]
	pub const fn encoder(self) -> &'static str {
		match self {
			Self::H264 => "libx264",
			Self::Hevc => "libx265",
		}
	}

	/// The codec's name as ffprobe reports it.
	#[must_use]
	pub const fn name(self) -> &'static str {
		match self {
			Self::H264 => "h264",
			Self::Hevc => "hevc",
		}
	}

	/// The encoder's profile option for ffprobe's profile name, when the
	/// encoder knows it.
	fn profile(self, probed: &str) -> Option<&'static str> {
		let name = probed.to_ascii_lowercase();
		match self {
			Self::H264 => {
				if name.contains("baseline") {
					Some("baseline")
				} else if name.contains("4:4:4") {
					Some("high444")
				} else if name.contains("4:2:2") {
					Some("high422")
				} else if name.contains("high 10") {
					Some("high10")
				} else if name == "high" {
					Some("high")
				} else if name == "main" || name == "extended" {
					Some("main")
				} else {
					None
				}
			}
			Self::Hevc => match name.as_str() {
				"main" => Some("main"),
				"main 10" => Some("main10"),
				_ => None,
			},
		}
	}
}

/// Why a file cannot be smart cut.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unsupported {
	/// The file has no video stream.
	#[error("the file has no video stream; a frame-accurate cut re-encodes video")]
	NoVideo,
	/// The video is not H.264 or H.265.
	#[error("the video is {codec}, and a smart cut can only re-encode H.264 and H.265 (HEVC)")]
	Codec {
		/// ffprobe's codec name.
		codec: String,
	},
	/// The video is interlaced.
	#[error("the video is interlaced ({field_order}), which the re-encoded parts would not be")]
	Interlaced {
		/// ffprobe's `field_order`.
		field_order: String,
	},
	/// The ffmpeg build lacks the encoder.
	#[error("this ffmpeg build has no {encoder} encoder, which a smart cut of {codec} needs")]
	NoEncoder {
		/// The encoder name.
		encoder: &'static str,
		/// The codec name.
		codec: &'static str,
	},
	/// A keyframe at a cut has frames after it in decode order that are
	/// shown before it (an open GOP).
	#[error(
		"the video has open GOPs (frames after a keyframe that are shown before it), which cannot be joined to a re-encoded part"
	)]
	OpenGop,
}

/// Why a smart cut cannot be planned.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlanError {
	/// See [`Unsupported`].
	#[error(transparent)]
	Unsupported(#[from] Unsupported),
	/// No frame was listed at or after a range's start.
	#[error("range {number} ({}) holds no frame", super::timecode::format(super::secs(*start)))]
	NoFrame {
		/// The range's number.
		number: usize,
		/// Its start.
		start: f64,
	},
	/// The packets read around the cuts do not reach a keyframe, so the
	/// range may hold one that was not seen. The caller retries with the
	/// packets of the whole range ([`whole_range_windows`]).
	#[error("range {number} has no keyframe within reach of its cuts; read the whole range")]
	ReadWholeRange {
		/// The range's number.
		number: usize,
	},
}

/// The video stream as the encoder must reproduce it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Video {
	/// The stream's index, what `-map 0:<index>` refers to.
	pub stream: u32,
	/// The codec, and so the encoder.
	pub codec: Codec,
	/// The encoder's profile, when the source's is known to it.
	pub profile: Option<&'static str>,
	/// The source's level as ffprobe reports it (`31` for 3.1), passed to
	/// x264. x265 picks its own: it refuses a level it cannot meet.
	pub level: Option<i64>,
	/// Whether the source reorders frames (B-frames). Without, the parts
	/// are encoded without B-frames too, so the decoder's delay does not
	/// change at the joins.
	pub b_frames: bool,
}

impl Video {
	fn from_stream(stream: &Stream, codec: Codec) -> Self {
		Self {
			stream: stream.index,
			codec,
			profile: stream
				.profile
				.as_deref()
				.and_then(|name| codec.profile(name)),
			level: stream.level.filter(|l| *l > 0),
			b_frames: stream.has_b_frames.unwrap_or(0) > 0,
		}
	}

	/// The encoder options of a part, from `-c:v` on.
	fn encoder_options(&self, cmd: FfmpegCommand) -> FfmpegCommand {
		let mut cmd = cmd
			.option("-c:v", self.codec.encoder())
			.option("-preset", PRESET);
		match self.codec {
			Codec::H264 => {
				cmd = cmd.option("-crf", CRF_H264.to_string());
				if let Some(profile) = self.profile {
					cmd = cmd.option("-profile:v", profile);
				}
				if let Some(level) = self.level {
					cmd = cmd.option("-level", level.to_string());
				}
				if !self.b_frames {
					cmd = cmd.option("-bf", "0");
				}
			}
			Codec::Hevc => {
				cmd = cmd.option("-crf", CRF_HEVC.to_string());
				if let Some(profile) = self.profile {
					cmd = cmd.option("-profile:v", profile);
				}
				let params = if self.b_frames {
					"log-level=error"
				} else {
					"log-level=error:bframes=0"
				};
				cmd = cmd.option("-x265-params", params);
			}
		}
		cmd
	}
}

/// Whether `probe` can be smart cut: its video is H.264 or H.265,
/// progressive, and `has_encoder` knows the encoder. The video stream is
/// the one [`Source::from_probe`] chose.
///
/// # Errors
/// [`Unsupported`], with the reason.
pub fn check(
	probe: &ProbeResult,
	source: &Source,
	has_encoder: impl Fn(&str) -> bool,
) -> Result<Video, Unsupported> {
	let stream = source
		.video_stream
		.and_then(|index| probe.streams.iter().find(|s| s.index == index))
		.filter(|s| s.codec_type == CodecType::Video)
		.ok_or(Unsupported::NoVideo)?;
	let name = stream.codec_name.as_deref().unwrap_or("unknown");
	let codec = Codec::from_name(name).ok_or_else(|| Unsupported::Codec {
		codec: name.to_owned(),
	})?;
	if let Some(order) = stream
		.field_order
		.as_deref()
		.filter(|o| !matches!(*o, "progressive" | "unknown" | ""))
	{
		return Err(Unsupported::Interlaced {
			field_order: order.to_owned(),
		});
	}
	if !has_encoder(codec.encoder()) {
		return Err(Unsupported::NoEncoder {
			encoder: codec.encoder(),
			codec: codec.name(),
		});
	}
	Ok(Video::from_stream(stream, codec))
}

/// The streams of `probe` a smart cut copies next to the video: audio,
/// subtitles and attachments. Data streams and further video streams
/// (cover art, a second angle) are left out.
#[must_use]
pub fn copied_streams(probe: &ProbeResult) -> Vec<u32> {
	probe
		.streams
		.iter()
		.filter(|s| {
			matches!(
				s.codec_type,
				CodecType::Audio | CodecType::Subtitle | CodecType::Attachment
			)
		})
		.map(|s| s.index)
		.collect()
}

/// The intervals of the container's timestamps whose packets [`plan`]
/// needs for the ranges of `base`, as
/// [`Runner::packets_in`](crate::ffmpeg::Runner::packets_in) takes them:
/// `window` seconds after each range's start and before its end, which
/// must reach the first and the last keyframe inside the range. When they
/// do not ([`PlanError::ReadWholeRange`]), retry with
/// [`whole_range_windows`].
#[must_use]
pub fn windows(base: &super::Plan, source: &Source, window: f64) -> Vec<(f64, f64)> {
	base.segments
		.iter()
		.flat_map(|s| {
			let start = base.start_time + s.segment.start;
			let end = base.start_time + s.segment.end;
			if end - start <= 2.0 * window {
				vec![(start - SNAP_TOLERANCE, end + END_MARGIN)]
			} else {
				vec![
					(start - SNAP_TOLERANCE, start + window),
					(end - window, end + END_MARGIN),
				]
			}
		})
		.map(|(from, to)| clamp(from, to, source))
		.collect()
}

/// Each range as one interval, for the retry.
#[must_use]
pub fn whole_range_windows(base: &super::Plan, source: &Source) -> Vec<(f64, f64)> {
	base.segments
		.iter()
		.map(|s| {
			clamp(
				base.start_time + s.segment.start - SNAP_TOLERANCE,
				base.start_time + s.segment.end + END_MARGIN,
				source,
			)
		})
		.collect()
}

fn clamp(from: f64, to: f64, source: &Source) -> (f64, f64) {
	let first = source.start_time.max(0.0);
	let last = source.start_time + source.duration;
	let from = from.max(first);
	(from, to.min(last).max(from))
}

/// One part of a range's video.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
	/// The scratch file.
	pub path: PathBuf,
	/// Encoded or copied.
	pub kind: PieceKind,
	/// The first frame's presentation time, relative to the start of the
	/// file.
	pub start: f64,
	/// Where the part ends (exclusive), relative to the start of the file:
	/// the next part's first frame, or the range's end.
	pub end: f64,
}

impl Piece {
	/// `end - start`.
	#[must_use]
	pub fn length(&self) -> f64 {
		self.end - self.start
	}
}

/// How a [`Piece`] is made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PieceKind {
	/// Decoded and encoded again: the frames from `start` to `end`.
	Encode,
	/// Copied: the packets from the first keyframe's decode time to the
	/// next keyframe's.
	Copy {
		/// The first keyframe's decode time, relative to the file start.
		from_dts: f64,
		/// The decode time of the keyframe after the last copied packet.
		to_dts: f64,
	},
}

/// One kept range as it will be cut.
#[derive(Debug, Clone, PartialEq)]
pub struct Range {
	/// The range's number in the output, from 1.
	pub number: usize,
	/// The kept range, snapped to frames: it starts on a frame and ends
	/// before the frame at its end.
	pub segment: Segment,
	/// The video parts in order.
	pub pieces: Vec<Piece>,
	/// The scratch file with the copied other streams, when there are any.
	pub streams: Option<PathBuf>,
	/// The file this range goes to.
	pub output: PathBuf,
}

impl Range {
	/// How many seconds of video are encoded again.
	#[must_use]
	pub fn encoded_seconds(&self) -> f64 {
		self.pieces
			.iter()
			.filter(|p| p.kind == PieceKind::Encode)
			.map(Piece::length)
			.sum()
	}

	/// `end - start`.
	#[must_use]
	pub fn length(&self) -> f64 {
		self.segment.length()
	}
}

/// A smart cut resolved against its source: what the job will do.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
	/// The kept ranges in output order.
	pub ranges: Vec<Range>,
	/// See [`Export`].
	pub export: Export,
	/// The folder the pieces are written to. It must exist when the job
	/// runs and is the job's to remove afterwards.
	pub scratch_dir: PathBuf,
	/// The video stream and how to encode it.
	pub video: Video,
	/// The streams copied next to the video; see [`copied_streams`].
	pub copied_streams: Vec<u32>,
	/// See [`Source::start_time`].
	pub start_time: f64,
}

/// Resolves the ranges of `base` (a [`super::plan`] in
/// [`CutMode::Smart`](super::CutMode::Smart)) against the video packets
/// around their cuts, as [`windows`] asked for them (`window` is the
/// window used, `None` after [`whole_range_windows`]). The pieces go to
/// `scratch_dir`.
///
/// # Errors
/// See [`PlanError`].
pub fn plan(
	base: &super::Plan,
	source: &Source,
	video: Video,
	copied_streams: Vec<u32>,
	packets: &[Packet],
	window: Option<f64>,
	scratch_dir: &Path,
) -> Result<Plan, PlanError> {
	let packets = normalize(packets, source);
	let mut ranges = Vec::with_capacity(base.segments.len());
	for segment in &base.segments {
		ranges.push(plan_range(
			segment,
			&packets,
			window,
			scratch_dir,
			!copied_streams.is_empty(),
		)?);
	}
	Ok(Plan {
		ranges,
		export: base.export,
		scratch_dir: scratch_dir.to_path_buf(),
		video,
		copied_streams,
		start_time: source.start_time,
	})
}

/// A packet relative to the start of the file, with a decode time (the
/// presentation time less the reorder delay when the container has none).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
	pts: f64,
	dts: f64,
	key: bool,
}

/// The packets relative to the start of the file, in decode order,
/// without the duplicates of overlapping windows.
fn normalize(packets: &[Packet], source: &Source) -> Vec<Frame> {
	let mut frames: Vec<Frame> = packets
		.iter()
		.map(|p| Frame {
			pts: p.pts - source.start_time,
			dts: p.dts.unwrap_or(p.pts - source.reorder_delay) - source.start_time,
			key: p.key,
		})
		.collect();
	frames.sort_by(|a, b| a.dts.total_cmp(&b.dts).then(a.pts.total_cmp(&b.pts)));
	frames.dedup_by(|a, b| (a.pts - b.pts).abs() < SNAP_TOLERANCE);
	frames
}

fn plan_range(
	segment: &PlannedSegment,
	frames: &[Frame],
	window: Option<f64>,
	scratch_dir: &Path,
	has_streams: bool,
) -> Result<Range, PlanError> {
	let number = segment.number;
	let Segment { start, end } = segment.segment;
	// Presentation order.
	let mut shown: Vec<f64> = frames.iter().map(|f| f.pts).collect();
	shown.sort_by(f64::total_cmp);
	let first_at = |time: f64| shown.iter().copied().find(|p| *p >= time - SNAP_TOLERANCE);
	let start = first_at(start).ok_or(PlanError::NoFrame { number, start })?;
	// A range at the end of the file has no frame at its end.
	let end = first_at(end).unwrap_or(end);
	if end <= start + SNAP_TOLERANCE {
		return Err(PlanError::NoFrame { number, start });
	}
	// The keyframes from the start to the end, the one at the end included:
	// the body can be copied up to the decode time of that one.
	let keyframes: Vec<&Frame> = frames
		.iter()
		.filter(|f| f.key && f.pts >= start - SNAP_TOLERANCE && f.pts <= end + SNAP_TOLERANCE)
		.collect();
	let file = |name: &str| scratch_dir.join(format!("range-{number}-{name}"));
	let encode = |index: usize, start: f64, end: f64| Piece {
		path: file(&format!("{index}-encode.ts")),
		kind: PieceKind::Encode,
		start,
		end,
	};
	let mut pieces = Vec::new();
	match (keyframes.first(), keyframes.last()) {
		(Some(first), Some(last))
			if first.pts < end - SNAP_TOLERANCE && last.pts > first.pts + SNAP_TOLERANCE =>
		{
			if let Some(window) = window
				&& end - start > 2.0 * window
				&& (first.pts > start + window || last.pts < end - window)
			{
				return Err(PlanError::ReadWholeRange { number });
			}
			// Frames decoded after a keyframe but shown before it cannot
			// follow a re-encoded head.
			let leading = frames
				.iter()
				.any(|f| f.dts > first.dts && f.pts < first.pts - SNAP_TOLERANCE);
			if leading {
				return Err(Unsupported::OpenGop.into());
			}
			// The body ends before the last keyframe's decode time, so the
			// tail begins with the earliest frame of the range decoded from
			// there on: that keyframe, or one shown before it.
			let tail_start = frames
				.iter()
				.filter(|f| f.dts >= last.dts && f.pts < end - SNAP_TOLERANCE)
				.map(|f| f.pts)
				.fold(end, f64::min);
			if first.pts > start + SNAP_TOLERANCE {
				pieces.push(encode(pieces.len(), start, first.pts));
			}
			pieces.push(Piece {
				path: file(&format!("{}-copy.ts", pieces.len())),
				kind: PieceKind::Copy {
					from_dts: first.dts,
					to_dts: last.dts,
				},
				start: first.pts,
				end: tail_start,
			});
			if end > tail_start + SNAP_TOLERANCE {
				pieces.push(encode(pieces.len(), tail_start, end));
			}
		}
		_ => {
			// No keyframe, or only one, inside: nothing worth copying.
			if let Some(window) = window
				&& end - start > 2.0 * window
			{
				return Err(PlanError::ReadWholeRange { number });
			}
			pieces.push(encode(0, start, end));
		}
	}
	Ok(Range {
		number,
		segment: Segment::new(start, end),
		pieces,
		streams: has_streams.then(|| file("streams.mkv")),
		output: segment.output.clone(),
	})
}

impl Plan {
	/// The files the job writes, in order, each once.
	#[must_use]
	pub fn outputs(&self) -> Vec<PathBuf> {
		let mut outputs: Vec<PathBuf> = Vec::new();
		for range in &self.ranges {
			if !outputs.contains(&range.output) {
				outputs.push(range.output.clone());
			}
		}
		outputs
	}

	/// How many seconds of video are encoded again, over all ranges.
	#[must_use]
	pub fn encoded_seconds(&self) -> f64 {
		self.ranges.iter().map(Range::encoded_seconds).sum()
	}

	/// The commands of the job, in order: [`Plan::prepare`] then
	/// [`Plan::assemble`].
	#[must_use]
	pub fn commands(&self, input: &Path) -> Vec<FfmpegCommand> {
		vec![self.prepare(input), self.assemble()]
	}

	/// The first command: writes every piece of every range from `input`.
	/// Each piece is an output of its own with its own input, seeked
	/// [`SEEK_MARGIN`] before the piece so the demuxer lands before it
	/// (an input seek counts from the start of the file), then trimmed
	/// exactly with `-copyts`, an output `-ss` and `-t` (which count in
	/// the container's own timestamps).
	#[must_use]
	pub fn prepare(&self, input: &Path) -> FfmpegCommand {
		let mut cmd = FfmpegCommand::new();
		let mut inputs = 0usize;
		let mut seek = |cmd: FfmpegCommand, at: f64| -> (FfmpegCommand, usize) {
			let index = inputs;
			inputs += 1;
			let seek = at - SEEK_MARGIN;
			let cmd = if seek > 0.0 {
				cmd.option("-ss", secs(seek))
			} else {
				cmd
			};
			(cmd.input(input), index)
		};
		let mut outputs: Vec<(usize, Output<'_>)> = Vec::new();
		for range in &self.ranges {
			for piece in &range.pieces {
				let (next, index) = seek(cmd, piece.start);
				cmd = next;
				outputs.push((index, Output::Piece(piece)));
			}
			if let Some(streams) = &range.streams {
				let (next, index) = seek(cmd, range.segment.start);
				cmd = next;
				outputs.push((index, Output::Streams(range, streams)));
			}
		}
		cmd = cmd.flag("-copyts");
		for (index, output) in outputs {
			cmd = match output {
				Output::Piece(piece) => {
					let cmd = cmd.option("-map", format!("{index}:{}", self.video.stream));
					match piece.kind {
						PieceKind::Encode => self.video.encoder_options(
							cmd.option("-ss", secs(self.start_time + piece.start - SNAP_TOLERANCE))
								.option("-t", secs(piece.length())),
						),
						PieceKind::Copy { from_dts, to_dts } => cmd
							.option("-ss", secs(self.start_time + from_dts - DTS_SLACK))
							.option("-t", secs(to_dts - from_dts))
							.option("-c:v", "copy"),
					}
					.option("-f", "mpegts")
					.output(&piece.path)
				}
				Output::Streams(range, path) => {
					let mut cmd = cmd;
					for stream in &self.copied_streams {
						cmd = cmd.option("-map", format!("{index}:{stream}"));
					}
					cmd.option("-ss", secs(self.start_time + range.segment.start))
						.option("-t", secs(range.length()))
						.option("-c", "copy")
						.option("-f", "matroska")
						.output(path)
				}
			};
		}
		cmd
	}

	/// The second command: joins the pieces into the output file(s) with
	/// the concat demuxer, the video pieces through one list and the
	/// copied streams through another, both inside the command as `data:`
	/// URLs (see [`Plan::concat_lists`]). The `file:` protocol must be
	/// allowed explicitly, because a list that is not itself a file may
	/// only open what its whitelist names.
	#[must_use]
	pub fn assemble(&self) -> FfmpegCommand {
		let mut cmd = FfmpegCommand::new();
		let mut maps: Vec<(PathBuf, Vec<usize>)> = Vec::new();
		let mut inputs = 0usize;
		for (output, video, streams) in self.concat_lists() {
			let mut indices = Vec::new();
			for list in [Some(video), streams].into_iter().flatten() {
				cmd = cmd
					.option("-f", "concat")
					.option("-safe", "0")
					.option("-protocol_whitelist", "data,file")
					.input(format!("data:text/plain,{list}"));
				indices.push(inputs);
				inputs += 1;
			}
			maps.push((output, indices));
		}
		// Without `-copyts` ffmpeg would move each input to start at zero,
		// and the copied streams begin a packet after the range's start.
		cmd = cmd.flag("-copyts");
		for (output, indices) in maps {
			for index in &indices {
				cmd = cmd.option("-map", index.to_string());
			}
			let metadata = indices
				.last()
				.map_or(-1, |i| i64::try_from(*i).unwrap_or(-1));
			cmd = cmd
				.option("-c", "copy")
				.option("-map_metadata", metadata.to_string())
				.option("-map_chapters", "-1")
				.option("-avoid_negative_ts", "make_zero")
				.output(output);
		}
		cmd
	}

	/// The concat lists of each output: the video pieces, then the copied
	/// streams (`None` without any). Each entry carries its `duration`, so
	/// the next one follows exactly where it ends; the copied streams also
	/// carry `inpoint 0`, so their timestamps are taken as they are.
	#[must_use]
	pub fn concat_lists(&self) -> Vec<(PathBuf, String, Option<String>)> {
		let mut lists: Vec<(PathBuf, String, Option<String>)> = Vec::new();
		for range in &self.ranges {
			let position = lists
				.iter()
				.position(|(output, ..)| *output == range.output)
				.unwrap_or_else(|| {
					lists.push((range.output.clone(), String::new(), None));
					lists.len() - 1
				});
			let entry = &mut lists[position];
			for piece in &range.pieces {
				// Writing to a String cannot fail.
				let _ = write!(
					entry.1,
					"file '{}'\nduration {}\n",
					file_url(&piece.path),
					secs(piece.length())
				);
			}
			if let Some(streams) = &range.streams {
				let list = entry.2.get_or_insert_with(String::new);
				let _ = write!(
					list,
					"file '{}'\ninpoint 0\nduration {}\n",
					file_url(streams),
					secs(range.length())
				);
			}
		}
		lists
	}
}

enum Output<'a> {
	Piece(&'a Piece),
	Streams(&'a Range, &'a Path),
}

/// A path as a `file:` URL in a concat list, with `'` escaped as a shell
/// would.
fn file_url(path: &Path) -> String {
	format!("file:{}", path.to_string_lossy()).replace('\'', r"'\''")
}

/// Seconds with microsecond precision and no trailing zeros, as ffmpeg
/// reads them. Negative values stay negative: an output `-ss` before zero
/// keeps a first packet whose decode time is negative, as a keyframe's is
/// in a container with an edit list.
fn secs(seconds: f64) -> String {
	let text = format!("{seconds:.6}");
	let text = text.trim_end_matches('0').trim_end_matches('.');
	if text.is_empty() || text == "-0" {
		"0".to_owned()
	} else {
		text.to_owned()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::cut::{CutMode, Request, SegmentMode};

	fn source() -> Source {
		Source {
			duration: 10.0,
			start_time: 0.0,
			video_stream: Some(0),
			reorder_delay: 0.08,
			video_start: 0.0,
			format_name: "mov,mp4,m4a,3gp,3g2,mj2".into(),
			extension: Some("mp4".into()),
		}
	}

	/// Ten seconds at 25 fps with a keyframe every second and two B-frames
	/// between P-frames, in decode order as x264 writes it: the keyframe,
	/// then each P-frame before the two B-frames it closes. The decode
	/// time runs two frames behind, so the first one is negative.
	fn packets(frames: i32) -> Vec<Packet> {
		let pts = |i: i32| f64::from(i) / 25.0;
		let mut order: Vec<i32> = Vec::new();
		for gop in (0..frames).step_by(25) {
			order.push(gop);
			let mut n = gop + 1;
			while n < (gop + 25).min(frames) {
				let p = (n + 2).min(gop + 24).min(frames - 1);
				order.push(p);
				order.extend(n..p);
				n = p + 1;
			}
		}
		order
			.iter()
			.enumerate()
			.map(|(position, &n)| Packet {
				pts: pts(n),
				dts: Some(pts(i32::try_from(position).unwrap()) - 0.08),
				key: n % 25 == 0,
			})
			.collect()
	}

	fn at(packets: &mut [Packet], pts: f64) -> &mut Packet {
		packets
			.iter_mut()
			.find(|p| (p.pts - pts).abs() < 1e-9)
			.unwrap()
	}

	fn video() -> Video {
		Video {
			stream: 0,
			codec: Codec::H264,
			profile: Some("high"),
			level: Some(31),
			b_frames: true,
		}
	}

	fn base(segments: &[(f64, f64)], export: Export) -> crate::cut::Plan {
		let request = Request {
			segments: segments.iter().map(|(s, e)| Segment::new(*s, *e)).collect(),
			segment_mode: SegmentMode::Keep,
			cut_mode: CutMode::Smart,
			export,
			output: PathBuf::from("/media/out/cut.mp4"),
		};
		crate::cut::plan(&request, &source(), &[]).unwrap()
	}

	fn planned(segments: &[(f64, f64)], export: Export) -> Plan {
		plan(
			&base(segments, export),
			&source(),
			video(),
			vec![1, 2],
			&packets(250),
			None,
			Path::new("/media/out/.scratch"),
		)
		.unwrap()
	}

	#[test]
	fn a_range_is_head_body_and_tail() {
		let plan = planned(&[(3.3, 8.5)], Export::SeparateFiles);
		assert_eq!(plan.ranges.len(), 1);
		let range = &plan.ranges[0];
		// 3.3 and 8.5 are between frames: the range runs from the first
		// frame at or after 3.3 to before the first frame at or after 8.5.
		assert!((range.segment.start - 3.32).abs() < 1e-9, "{range:?}");
		assert!((range.segment.end - 8.52).abs() < 1e-9);
		let kinds: Vec<(PieceKind, f64, f64)> = range
			.pieces
			.iter()
			.map(|p| (p.kind, p.start, p.end))
			.collect();
		assert_eq!(kinds.len(), 3, "{kinds:?}");
		assert_eq!(kinds[0].0, PieceKind::Encode);
		assert!((kinds[0].1 - 3.32).abs() < 1e-9 && (kinds[0].2 - 4.0).abs() < 1e-9);
		assert!(
			matches!(kinds[1].0, PieceKind::Copy { from_dts, to_dts }
				if (from_dts - 3.92).abs() < 1e-9 && (to_dts - 7.92).abs() < 1e-9),
			"{kinds:?}"
		);
		assert!((kinds[1].1 - 4.0).abs() < 1e-9 && (kinds[1].2 - 8.0).abs() < 1e-9);
		assert_eq!(kinds[2].0, PieceKind::Encode);
		assert!((kinds[2].1 - 8.0).abs() < 1e-9 && (kinds[2].2 - 8.52).abs() < 1e-9);
		assert!((range.encoded_seconds() - 1.2).abs() < 1e-9);
		assert_eq!(
			range.streams.as_deref(),
			Some(Path::new("/media/out/.scratch/range-1-streams.mkv"))
		);
		assert_eq!(plan.outputs(), [PathBuf::from("/media/out/cut-1.mp4")]);
	}

	#[test]
	fn cuts_on_keyframes_need_no_encoding() {
		let plan = planned(&[(2.0, 6.0)], Export::SeparateFiles);
		let range = &plan.ranges[0];
		assert_eq!(range.pieces.len(), 1, "{range:?}");
		assert!(matches!(range.pieces[0].kind, PieceKind::Copy { .. }));
		assert_eq!(range.encoded_seconds(), 0.0);
		// Within a millisecond counts as on the frame.
		let plan = planned(&[(1.9996, 6.0004)], Export::SeparateFiles);
		assert_eq!(plan.ranges[0].pieces.len(), 1);
		assert!((plan.ranges[0].segment.start - 2.0).abs() < 1e-9);
		assert!((plan.ranges[0].segment.end - 6.0).abs() < 1e-9);
	}

	#[test]
	fn a_range_within_a_gop_is_encoded_whole() {
		let plan = planned(&[(2.2, 2.8)], Export::SeparateFiles);
		let range = &plan.ranges[0];
		assert_eq!(range.pieces.len(), 1);
		assert_eq!(range.pieces[0].kind, PieceKind::Encode);
		assert!((range.encoded_seconds() - 0.6).abs() < 1e-9);
		// So is one that holds a single keyframe.
		let plan = planned(&[(2.5, 3.5)], Export::SeparateFiles);
		assert_eq!(plan.ranges[0].pieces.len(), 1);
		assert_eq!(plan.ranges[0].pieces[0].kind, PieceKind::Encode);
	}

	#[test]
	fn the_end_of_the_file_is_a_tail() {
		let plan = planned(&[(7.5, 10.0)], Export::SeparateFiles);
		let range = &plan.ranges[0];
		assert_eq!(range.pieces.len(), 3, "{range:?}");
		assert!((range.pieces[0].start - 7.52).abs() < 1e-9);
		assert!((range.pieces[2].start - 9.0).abs() < 1e-9);
		assert!((range.pieces[2].end - 10.0).abs() < 1e-9);
	}

	#[test]
	fn windows_cover_short_ranges_whole_and_long_ranges_at_both_ends() {
		let base = base(&[(1.0, 3.0), (3.0, 9.5)], Export::SeparateFiles);
		let windows = windows(&base, &source(), 2.0);
		assert_eq!(windows.len(), 3, "{windows:?}");
		assert!((windows[0].0 - 0.999).abs() < 1e-9 && (windows[0].1 - 3.5).abs() < 1e-9);
		assert!((windows[1].0 - 2.999).abs() < 1e-9 && (windows[1].1 - 5.0).abs() < 1e-9);
		assert!((windows[2].0 - 7.5).abs() < 1e-9 && (windows[2].1 - 10.0).abs() < 1e-9);
		let whole = whole_range_windows(&base, &source());
		assert_eq!(whole.len(), 2);
		assert!((whole[1].1 - 10.0).abs() < 1e-9);
	}

	#[test]
	fn a_long_gop_asks_for_the_whole_range() {
		// Keyframes only at 0 and 9: with a 2 s window around the cuts of
		// [1, 8.5) none is seen.
		let mut packets = packets(250);
		for p in &mut packets {
			p.key = p.pts.abs() < 1e-9 || (p.pts - 9.0).abs() < 1e-9;
		}
		let base = base(&[(1.0, 8.5)], Export::SeparateFiles);
		let seen: Vec<Packet> = packets
			.iter()
			.filter(|p| p.pts < 3.0 || p.pts > 6.5)
			.copied()
			.collect();
		let err = plan(
			&base,
			&source(),
			video(),
			vec![],
			&seen,
			Some(2.0),
			Path::new("/s"),
		)
		.unwrap_err();
		assert_eq!(err, PlanError::ReadWholeRange { number: 1 });
		// With the whole range read, the range is encoded whole.
		let plan = plan(
			&base,
			&source(),
			video(),
			vec![],
			&packets,
			None,
			Path::new("/s"),
		)
		.unwrap();
		assert_eq!(plan.ranges[0].pieces.len(), 1);
		assert_eq!(plan.ranges[0].streams, None);
	}

	#[test]
	fn open_gops_are_refused_and_tails_start_at_the_gop_boundary() {
		// Two frames after the keyframe at 4.0 in decode order are shown
		// before it.
		let mut packets = packets(250);
		at(&mut packets, 3.96).dts = Some(4.0);
		at(&mut packets, 3.92).dts = Some(4.0);
		let err = plan(
			&base(&[(3.3, 8.5)], Export::SeparateFiles),
			&source(),
			video(),
			vec![],
			&packets,
			None,
			Path::new("/s"),
		)
		.unwrap_err();
		assert_eq!(err, PlanError::Unsupported(Unsupported::OpenGop));
		// Shown-before frames of the last keyframe go to the tail instead.
		let plan = plan(
			&base(&[(4.5, 8.5)], Export::SeparateFiles),
			&source(),
			video(),
			vec![],
			&packets,
			None,
			Path::new("/s"),
		)
		.unwrap();
		let mut packets = self::packets(250);
		at(&mut packets, 7.96).dts = Some(8.0);
		let plan2 = self::plan(
			&base(&[(4.5, 8.5)], Export::SeparateFiles),
			&source(),
			video(),
			vec![],
			&packets,
			None,
			Path::new("/s"),
		)
		.unwrap();
		assert!((plan.ranges[0].pieces[1].end - 8.0).abs() < 1e-9);
		assert!((plan2.ranges[0].pieces[1].end - 7.96).abs() < 1e-9);
		assert!((plan2.ranges[0].pieces[2].start - 7.96).abs() < 1e-9);
	}

	#[test]
	fn a_cut_past_the_last_frame_is_an_error() {
		let err = plan(
			&base(&[(9.99, 10.0)], Export::SeparateFiles),
			&source(),
			video(),
			vec![],
			&packets(249),
			None,
			Path::new("/s"),
		)
		.unwrap_err();
		assert!(matches!(err, PlanError::NoFrame { number: 1, .. }), "{err}");
		assert_eq!(err.to_string(), "range 1 (0:00:09.990) holds no frame");
	}

	#[test]
	fn the_commands_of_separate_files() {
		let plan = planned(&[(3.3, 8.5), (0.0, 1.5)], Export::SeparateFiles);
		assert_eq!(plan.ranges[0].number, 1);
		assert!((plan.ranges[0].segment.start).abs() < 1e-9);
		insta::assert_snapshot!(plan.prepare(Path::new("/media/in/it's a movie.mp4")));
		insta::assert_snapshot!(plan.assemble());
		let lists = plan.concat_lists();
		assert_eq!(lists.len(), 2);
		insta::assert_snapshot!(lists[1].1);
		insta::assert_snapshot!(lists[1].2.as_deref().unwrap_or_default());
	}

	#[test]
	fn a_merged_export_joins_the_ranges_in_one_list() {
		let plan = planned(&[(3.3, 5.0), (7.0, 8.5)], Export::Merged);
		assert_eq!(plan.outputs(), [PathBuf::from("/media/out/cut.mp4")]);
		let lists = plan.concat_lists();
		assert_eq!(lists.len(), 1);
		let (_, video, streams) = &lists[0];
		assert_eq!(video.matches("file ").count(), 4, "{video}");
		assert_eq!(streams.as_deref().unwrap().matches("inpoint 0").count(), 2);
		insta::assert_snapshot!(plan.assemble());
		assert_eq!(plan.commands(Path::new("/in.mp4")).len(), 2);
	}

	#[test]
	fn a_video_only_source_has_no_streams_piece() {
		let plan = plan(
			&base(&[(3.3, 8.5)], Export::SeparateFiles),
			&source(),
			video(),
			vec![],
			&packets(250),
			None,
			Path::new("/media/out/.scratch"),
		)
		.unwrap();
		assert_eq!(plan.ranges[0].streams, None);
		let lists = plan.concat_lists();
		assert_eq!(lists[0].2, None);
		let cmd = plan.assemble().to_string();
		assert!(cmd.contains("-map 0 -c copy -map_metadata 0"), "{cmd}");
		assert_eq!(cmd.matches("-f concat").count(), 1, "{cmd}");
	}

	#[test]
	fn hevc_without_b_frames_encodes_accordingly() {
		let video = Video {
			stream: 2,
			codec: Codec::Hevc,
			profile: Some("main10"),
			level: Some(120),
			b_frames: false,
		};
		let cmd = video.encoder_options(FfmpegCommand::new()).to_string();
		assert_eq!(
			cmd,
			"ffmpeg -c:v libx265 -preset medium -crf 20 -profile:v main10 -x265-params log-level=error:bframes=0"
		);
		let video = Video {
			codec: Codec::H264,
			profile: None,
			..video
		};
		let cmd = video.encoder_options(FfmpegCommand::new()).to_string();
		assert_eq!(
			cmd,
			"ffmpeg -c:v libx264 -preset medium -crf 18 -level 120 -bf 0"
		);
	}

	#[test]
	fn profiles_map_to_the_encoders_names() {
		assert_eq!(Codec::H264.profile("High"), Some("high"));
		assert_eq!(
			Codec::H264.profile("Constrained Baseline"),
			Some("baseline")
		);
		assert_eq!(Codec::H264.profile("High 10"), Some("high10"));
		assert_eq!(
			Codec::H264.profile("High 4:4:4 Predictive"),
			Some("high444")
		);
		assert_eq!(Codec::H264.profile("High 4:2:2 Intra"), Some("high422"));
		assert_eq!(Codec::H264.profile("Main"), Some("main"));
		assert_eq!(Codec::H264.profile("Multiview High"), None);
		assert_eq!(Codec::Hevc.profile("Main"), Some("main"));
		assert_eq!(Codec::Hevc.profile("Main 10"), Some("main10"));
		assert_eq!(Codec::Hevc.profile("Rext"), None);
	}

	#[test]
	fn the_check_names_the_reason() {
		let parse = |json: &str| crate::ffmpeg::probe::parse(json).unwrap();
		let source = |probe: &ProbeResult| Source::from_probe(probe, Path::new("x.mp4")).unwrap();
		let all = |_: &str| true;

		let h264 = parse(
			r#"{"format":{"duration":"10"},"streams":[{"index":0,"codec_type":"video","codec_name":"h264","profile":"High","level":40,"has_b_frames":2,"field_order":"progressive"},{"index":1,"codec_type":"audio","codec_name":"aac"},{"index":2,"codec_type":"data"},{"index":3,"codec_type":"subtitle"}]}"#,
		);
		let video = check(&h264, &source(&h264), all).unwrap();
		assert_eq!(
			video,
			Video {
				stream: 0,
				codec: Codec::H264,
				profile: Some("high"),
				level: Some(40),
				b_frames: true
			}
		);
		assert_eq!(copied_streams(&h264), [1, 3]);
		assert_eq!(
			check(&h264, &source(&h264), |name| name != "libx264").unwrap_err(),
			Unsupported::NoEncoder {
				encoder: "libx264",
				codec: "h264"
			}
		);

		let audio = parse(
			r#"{"format":{"duration":"10"},"streams":[{"index":0,"codec_type":"audio","codec_name":"aac"}]}"#,
		);
		assert_eq!(
			check(&audio, &source(&audio), all).unwrap_err(),
			Unsupported::NoVideo
		);

		let vp9 = parse(
			r#"{"format":{"duration":"10"},"streams":[{"index":0,"codec_type":"video","codec_name":"vp9"}]}"#,
		);
		assert_eq!(
			check(&vp9, &source(&vp9), all).unwrap_err(),
			Unsupported::Codec {
				codec: "vp9".into()
			}
		);

		let interlaced = parse(
			r#"{"format":{"duration":"10"},"streams":[{"index":0,"codec_type":"video","codec_name":"hevc","field_order":"tt"}]}"#,
		);
		let err = check(&interlaced, &source(&interlaced), all).unwrap_err();
		assert_eq!(
			err,
			Unsupported::Interlaced {
				field_order: "tt".into()
			}
		);
		assert!(err.to_string().contains("interlaced (tt)"), "{err}");

		// Cover art is not the video stream.
		let cover = parse(
			r#"{"format":{"duration":"10"},"streams":[{"index":0,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}},{"index":1,"codec_type":"video","codec_name":"hevc","profile":"Main 10","has_b_frames":0}]}"#,
		);
		let video = check(&cover, &source(&cover), all).unwrap();
		assert_eq!(
			(video.stream, video.codec, video.profile),
			(1, Codec::Hevc, Some("main10"))
		);
		assert!(!video.b_frames);
	}

	#[test]
	fn seconds_are_compact() {
		assert_eq!(secs(3.32), "3.32");
		assert_eq!(secs(0.0), "0");
		assert_eq!(secs(-0.081), "-0.081");
		assert_eq!(secs(-1e-7), "0");
		assert_eq!(secs(1.0 / 3.0), "0.333333");
		assert_eq!(
			file_url(Path::new("/media/in/it's.mp4")),
			r"file:/media/in/it'\''s.mp4"
		);
	}
}
