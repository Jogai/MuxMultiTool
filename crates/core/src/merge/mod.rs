//! Lossless merge: joining files with identical codec parameters, after a
//! compatibility check.
//!
//! ffmpeg's concat demuxer glues files packet by packet without decoding,
//! which only works when every file has the same streams in the same order,
//! each with the same codec and parameters. ffmpeg does not check that: a
//! mismatch gives an output that plays the first part and then stutters,
//! shows garbage or goes silent. So [`check`] compares every stream across
//! the parts before anything runs and names what differs in which file, as
//! the compatibility table of the GUI sketch shows it (gui/06-merge).
//!
//! The first part is the reference. Per stream the codec is compared, then
//! for video the resolution, frame rate and pixel format, for audio the
//! sample rate and channel layout. A part whose streams are not of the same
//! kinds in the same order has a stream layout mismatch; the concat demuxer
//! cannot handle that at all. A stream that differs can be left out of the
//! merge ([`check`]'s `drop`), which keeps the merge lossless; every other
//! mismatch blocks it ([`Check::blocking`]).
//!
//! The command and its two sidecar files (the concat list, the chapters) are
//! in [`command`].

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ffmpeg::probe::{CodecType, ProbeResult, Rational, Stream};

pub mod command;

/// The fewest files a merge needs.
pub const MIN_PARTS: usize = 2;

/// One file to merge, as the check sees it: its name for the messages, its
/// duration for the chapters and the progress, and its streams.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
	/// The file.
	pub path: PathBuf,
	/// The file name, as the messages and the chapter titles call the part.
	pub name: String,
	/// The container's duration, when the probe knows it.
	pub duration: Option<Duration>,
	/// The streams in container order.
	pub streams: Vec<StreamParams>,
}

impl Part {
	/// The part at `path` as `probe` describes it.
	#[must_use]
	pub fn from_probe(path: &Path, probe: &ProbeResult) -> Self {
		Self {
			path: path.to_path_buf(),
			name: path
				.file_name()
				.map(|n| n.to_string_lossy().into_owned())
				.unwrap_or_default(),
			duration: probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
			streams: probe
				.streams
				.iter()
				.map(StreamParams::from_stream)
				.collect(),
		}
	}

	/// The stream kinds in order, e.g. `video, audio, audio, subtitle`: what
	/// the stream layout comparison looks at.
	#[must_use]
	pub fn layout(&self) -> String {
		self.streams
			.iter()
			.map(|s| s.kind.to_string())
			.collect::<Vec<_>>()
			.join(", ")
	}
}

/// The parameters of one stream that must match across the parts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamParams {
	/// Position in the container.
	pub index: u32,
	/// What the stream carries.
	pub kind: CodecType,
	/// Short codec name (`h264`, `aac`).
	pub codec: Option<String>,
	/// Picture width (video).
	pub width: Option<u32>,
	/// Picture height (video).
	pub height: Option<u32>,
	/// Frame rate (video).
	pub frame_rate: Option<Rational>,
	/// Pixel format (video).
	pub pix_fmt: Option<String>,
	/// Sample rate in Hz (audio).
	pub sample_rate: Option<u32>,
	/// Channel count (audio).
	pub channels: Option<u32>,
	/// Channel layout (audio).
	pub channel_layout: Option<String>,
	/// The `language` tag, unless it is `und` (undetermined).
	pub language: Option<String>,
}

impl StreamParams {
	/// The parameters of `stream`.
	#[must_use]
	pub fn from_stream(stream: &Stream) -> Self {
		Self {
			index: stream.index,
			kind: stream.codec_type,
			codec: stream.codec_name.clone(),
			width: stream.width,
			height: stream.height,
			frame_rate: stream
				.avg_frame_rate
				.filter(|r| r.num != 0)
				.or(stream.r_frame_rate),
			pix_fmt: stream.pix_fmt.clone(),
			sample_rate: stream.sample_rate,
			channels: stream.channels,
			channel_layout: stream.channel_layout.clone(),
			language: stream
				.language()
				.filter(|l| *l != UNDETERMINED)
				.map(str::to_owned),
		}
	}

	/// The stream in a few words for the compatibility table, e.g.
	/// `h264 1920×1080 23.976`, `ac3 5.1 48 kHz` or `subrip eng`.
	#[must_use]
	pub fn summary(&self) -> String {
		let mut words = vec![self.codec.clone().unwrap_or_else(|| self.kind.to_string())];
		match self.kind {
			CodecType::Video => {
				words.extend(self.resolution());
				words.extend(self.frame_rate.and_then(Rational::as_f64).map(fps));
			}
			CodecType::Audio => {
				words.extend(self.layout());
				words.extend(self.sample_rate.map(khz));
			}
			_ => {}
		}
		words.extend(self.language.clone());
		words.join(" ")
	}

	/// `1920×1080`, when both sizes are known.
	fn resolution(&self) -> Option<String> {
		Some(format!("{}×{}", self.width?, self.height?))
	}

	/// The channel layout, or the channel count when the layout is unknown.
	fn layout(&self) -> Option<String> {
		self.channel_layout
			.clone()
			.or_else(|| self.channels.map(|n| format!("{n} ch")))
	}

	/// The value of `parameter` as the messages show it, `None` when unknown.
	fn value(&self, parameter: Parameter) -> Option<String> {
		match parameter {
			Parameter::Codec => self.codec.clone(),
			Parameter::Resolution => self.resolution(),
			Parameter::FrameRate => self.frame_rate.and_then(Rational::as_f64).map(fps),
			Parameter::PixelFormat => self.pix_fmt.clone(),
			Parameter::SampleRate => self.sample_rate.map(khz),
			Parameter::ChannelLayout => self.layout(),
			Parameter::StreamLayout => None,
		}
	}

	/// Whether `parameter` is the same in `self` and `other`. An unknown
	/// value on either side is taken as matching: the check names what it
	/// knows differs, it does not guess.
	fn same(&self, other: &Self, parameter: Parameter) -> bool {
		match parameter {
			Parameter::FrameRate => match (self.fps(), other.fps()) {
				(Some(a), Some(b)) => (a - b).abs() < FPS_TOLERANCE,
				_ => true,
			},
			_ => match (self.value(parameter), other.value(parameter)) {
				(Some(a), Some(b)) => a == b,
				_ => true,
			},
		}
	}

	fn fps(&self) -> Option<f64> {
		self.frame_rate.and_then(Rational::as_f64)
	}
}

/// The language tag that means "none given": MP4 writes it on every stream.
const UNDETERMINED: &str = "und";

/// Frame rates closer than this are the same: `30000/1001` and `2997/100`
/// both play as 29.97.
const FPS_TOLERANCE: f64 = 0.001;

/// A parameter the check compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Parameter {
	/// The codec, every stream kind.
	Codec,
	/// Width × height, video.
	Resolution,
	/// Frames per second, video.
	FrameRate,
	/// The pixel format, video.
	PixelFormat,
	/// Samples per second, audio.
	SampleRate,
	/// The channel layout (or count), audio.
	ChannelLayout,
	/// The kinds of streams and their order, per part rather than per
	/// stream.
	StreamLayout,
}

impl Parameter {
	/// The parameters compared for a stream of `kind`, in order.
	#[must_use]
	pub const fn for_kind(kind: CodecType) -> &'static [Self] {
		match kind {
			CodecType::Video => &[
				Self::Codec,
				Self::Resolution,
				Self::FrameRate,
				Self::PixelFormat,
			],
			CodecType::Audio => &[Self::Codec, Self::SampleRate, Self::ChannelLayout],
			_ => &[Self::Codec],
		}
	}
}

impl fmt::Display for Parameter {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::Codec => "codec",
			Self::Resolution => "resolution",
			Self::FrameRate => "frame rate",
			Self::PixelFormat => "pixel format",
			Self::SampleRate => "sample rate",
			Self::ChannelLayout => "channel layout",
			Self::StreamLayout => "stream layout",
		})
	}
}

/// One parameter that differs between a part and the first part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
	/// The stream's index in the first part. For a stream layout mismatch,
	/// the first index where the layouts diverge.
	pub stream: u32,
	/// Which part differs, as a position in the parts given to [`check`];
	/// never 0, the first part is the reference.
	pub part: usize,
	/// The name of that part.
	pub part_name: String,
	/// What differs.
	pub parameter: Parameter,
	/// The first part's value.
	pub expected: String,
	/// The differing part's value.
	pub actual: String,
}

/// `channel layout differs (E03.mkv): stereo instead of 5.1`.
impl fmt::Display for Mismatch {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{} differs ({}): {} instead of {}",
			self.parameter, self.part_name, self.actual, self.expected
		)
	}
}

/// One row of the compatibility table: a stream of the first part across
/// every part.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
	/// The stream's index in the first part.
	pub index: u32,
	/// What the stream carries.
	pub kind: CodecType,
	/// Per part, the stream at this index in a few words
	/// ([`StreamParams::summary`]), or `None` when the part has no such
	/// stream.
	pub cells: Vec<Option<String>>,
	/// Whether the stream is left out of the merge.
	pub dropped: bool,
	/// What differs, in part order; empty when the stream is identical
	/// across the parts.
	pub mismatches: Vec<Mismatch>,
}

impl Row {
	/// Whether the stream is the same in every part.
	#[must_use]
	pub fn identical(&self) -> bool {
		self.mismatches.is_empty()
	}
}

/// What the check found.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
	/// One row per stream of the first part.
	pub rows: Vec<Row>,
	/// The parts whose streams are not of the same kinds in the same order
	/// as the first part's, one mismatch each.
	pub layout: Vec<Mismatch>,
	/// The sum of the parts' durations, when every part has one: what the
	/// output should last.
	pub total_duration: Option<Duration>,
}

impl Check {
	/// Every mismatch that stops the merge: the layout mismatches and those
	/// of the streams that are not dropped.
	pub fn blocking(&self) -> impl Iterator<Item = &Mismatch> {
		self.layout.iter().chain(
			self.rows
				.iter()
				.filter(|row| !row.dropped)
				.flat_map(|row| row.mismatches.iter()),
		)
	}

	/// Whether the parts can be merged as they are.
	#[must_use]
	pub fn can_merge(&self) -> bool {
		self.blocking().next().is_none()
	}

	/// The reason the merge is blocked, one sentence per mismatch, joined
	/// with `; `. Empty when it can go ahead.
	#[must_use]
	pub fn reason(&self) -> String {
		self.blocking()
			.map(ToString::to_string)
			.collect::<Vec<_>>()
			.join("; ")
	}
}

/// Why a request cannot be checked at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
	/// Fewer than [`MIN_PARTS`] parts.
	#[error("a merge needs at least {MIN_PARTS} files")]
	TooFewParts,
	/// A dropped stream index that the first part does not have.
	#[error("stream #{0} is not a stream of the first file")]
	UnknownStream(u32),
	/// Every stream is dropped.
	#[error("every stream is left out; nothing would be merged")]
	NothingLeft,
}

/// Compares `parts` stream by stream against the first one. The streams
/// whose index is in `drop` are left out of the merge, so their mismatches
/// do not block it.
///
/// # Errors
/// See [`CheckError`]: the request is not a merge at all. A mismatch is not
/// an error; see [`Check::blocking`].
pub fn check(parts: &[Part], drop: &[u32]) -> Result<Check, CheckError> {
	let [first, rest @ ..] = parts else {
		return Err(CheckError::TooFewParts);
	};
	if rest.is_empty() {
		return Err(CheckError::TooFewParts);
	}
	if let Some(unknown) = drop
		.iter()
		.find(|d| !first.streams.iter().any(|s| s.index == **d))
	{
		return Err(CheckError::UnknownStream(*unknown));
	}
	if first.streams.iter().all(|s| drop.contains(&s.index)) {
		return Err(CheckError::NothingLeft);
	}

	let rows = first
		.streams
		.iter()
		.enumerate()
		.map(|(position, reference)| {
			let mut mismatches = Vec::new();
			let cells = parts
				.iter()
				.enumerate()
				.map(|(part, other)| {
					let stream = other.streams.get(position)?;
					if part > 0 && stream.kind == reference.kind {
						mismatches.extend(
							Parameter::for_kind(reference.kind)
								.iter()
								.filter(|p| !reference.same(stream, **p))
								.map(|p| Mismatch {
									stream: reference.index,
									part,
									part_name: other.name.clone(),
									parameter: *p,
									expected: reference.value(*p).unwrap_or_default(),
									actual: stream.value(*p).unwrap_or_default(),
								}),
						);
					}
					Some(stream.summary())
				})
				.collect();
			Row {
				index: reference.index,
				kind: reference.kind,
				cells,
				dropped: drop.contains(&reference.index),
				mismatches,
			}
		})
		.collect();

	let layout = rest
		.iter()
		.enumerate()
		.filter_map(|(i, part)| {
			let kinds = |p: &Part| p.streams.iter().map(|s| s.kind).collect::<Vec<_>>();
			let (reference, other) = (kinds(first), kinds(part));
			if reference == other {
				return None;
			}
			// The first index whose kinds differ, or where one side ends.
			let diverges = reference
				.iter()
				.zip(&other)
				.position(|(a, b)| a != b)
				.unwrap_or_else(|| reference.len().min(other.len()));
			Some(Mismatch {
				stream: u32::try_from(diverges).unwrap_or(u32::MAX),
				part: i + 1,
				part_name: part.name.clone(),
				parameter: Parameter::StreamLayout,
				expected: first.layout(),
				actual: part.layout(),
			})
		})
		.collect();

	let total_duration = parts
		.iter()
		.map(|p| p.duration)
		.try_fold(Duration::ZERO, |sum, d| Some(sum + d?));

	Ok(Check {
		rows,
		layout,
		total_duration,
	})
}

/// A frame rate for the table: `25`, or `23.976` when not whole.
fn fps(rate: f64) -> String {
	trim_zeros(format!("{rate:.3}"))
}

/// A sample rate for the table: `48 kHz`, `44.1 kHz`.
fn khz(rate: u32) -> String {
	format!(
		"{} kHz",
		trim_zeros(format!("{:.3}", f64::from(rate) / 1000.0))
	)
}

/// `25.000` → `25`, `23.976` stays, `44.100` → `44.1`.
fn trim_zeros(number: String) -> String {
	if number.contains('.') {
		number
			.trim_end_matches('0')
			.trim_end_matches('.')
			.to_owned()
	} else {
		number
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn video(width: u32, height: u32, fps: (i64, i64)) -> StreamParams {
		StreamParams {
			index: 0,
			kind: CodecType::Video,
			codec: Some("h264".into()),
			width: Some(width),
			height: Some(height),
			frame_rate: Some(Rational {
				num: fps.0,
				den: fps.1,
			}),
			pix_fmt: Some("yuv420p".into()),
			..StreamParams::default()
		}
	}

	fn audio(index: u32, codec: &str, layout: &str, rate: u32) -> StreamParams {
		StreamParams {
			index,
			kind: CodecType::Audio,
			codec: Some(codec.into()),
			sample_rate: Some(rate),
			channels: Some(if layout == "stereo" { 2 } else { 6 }),
			channel_layout: Some(layout.into()),
			language: Some("eng".into()),
			..StreamParams::default()
		}
	}

	fn subs(index: u32) -> StreamParams {
		StreamParams {
			index,
			kind: CodecType::Subtitle,
			codec: Some("subrip".into()),
			language: Some("eng".into()),
			..StreamParams::default()
		}
	}

	fn episode(n: u32, secs: u64, streams: Vec<StreamParams>) -> Part {
		Part {
			path: PathBuf::from(format!("/media/Show S01E0{n}.mkv")),
			name: format!("Show S01E0{n}.mkv"),
			duration: Some(Duration::from_secs(secs)),
			streams,
		}
	}

	fn standard() -> Vec<StreamParams> {
		vec![
			video(1920, 1080, (24000, 1001)),
			audio(1, "ac3", "5.1", 48_000),
			audio(2, "aac", "stereo", 48_000),
			subs(3),
		]
	}

	#[test]
	fn identical_parts_can_be_merged() {
		let parts = [
			episode(1, 2892, standard()),
			episode(2, 3100, standard()),
			episode(3, 3108, standard()),
		];
		let check = check(&parts, &[]).unwrap();
		assert!(check.can_merge());
		assert_eq!(check.reason(), "");
		assert_eq!(check.layout, []);
		assert_eq!(check.rows.len(), 4);
		assert!(check.rows.iter().all(Row::identical));
		assert_eq!(
			check.rows[0].cells,
			vec![Some("h264 1920×1080 23.976".to_owned()); 3]
		);
		assert_eq!(
			check.rows[1].cells[0].as_deref(),
			Some("ac3 5.1 48 kHz eng")
		);
		assert_eq!(
			check.rows[2].cells[0].as_deref(),
			Some("aac stereo 48 kHz eng")
		);
		assert_eq!(check.rows[3].cells[0].as_deref(), Some("subrip eng"));
		assert_eq!(check.total_duration, Some(Duration::from_secs(9100)));
	}

	#[test]
	fn a_mismatch_names_the_parameter_and_the_part() {
		let mut third = standard();
		third[1] = audio(1, "ac3", "stereo", 48_000);
		let parts = [
			episode(1, 10, standard()),
			episode(2, 10, standard()),
			episode(3, 10, third),
		];
		let check = check(&parts, &[]).unwrap();
		assert!(!check.can_merge());
		assert_eq!(
			check.rows[1].mismatches,
			vec![Mismatch {
				stream: 1,
				part: 2,
				part_name: "Show S01E03.mkv".into(),
				parameter: Parameter::ChannelLayout,
				expected: "5.1".into(),
				actual: "stereo".into(),
			}]
		);
		assert_eq!(
			check.reason(),
			"channel layout differs (Show S01E03.mkv): stereo instead of 5.1"
		);
		assert!(check.rows[0].identical());
		assert!(check.rows[2].identical());

		// Leaving the stream out makes the merge possible.
		let fixed = super::check(&parts, &[1]).unwrap();
		assert!(fixed.rows[1].dropped);
		assert!(!fixed.rows[1].identical(), "the table still shows it");
		assert!(fixed.can_merge());
	}

	#[test]
	fn every_video_and_audio_parameter_is_compared() {
		let mut other = standard();
		other[0] = StreamParams {
			codec: Some("hevc".into()),
			pix_fmt: Some("yuv420p10le".into()),
			..video(1280, 720, (25, 1))
		};
		other[1] = audio(1, "eac3", "5.1", 44_100);
		other[3] = StreamParams {
			codec: Some("ass".into()),
			..subs(3)
		};
		let parts = [episode(1, 10, standard()), episode(2, 10, other)];
		let check = check(&parts, &[]).unwrap();
		let found: Vec<(u32, Parameter)> =
			check.blocking().map(|m| (m.stream, m.parameter)).collect();
		assert_eq!(
			found,
			[
				(0, Parameter::Codec),
				(0, Parameter::Resolution),
				(0, Parameter::FrameRate),
				(0, Parameter::PixelFormat),
				(1, Parameter::Codec),
				(1, Parameter::SampleRate),
				(3, Parameter::Codec),
			]
		);
		let messages: Vec<String> = check.blocking().map(ToString::to_string).collect();
		assert_eq!(
			messages[1],
			"resolution differs (Show S01E02.mkv): 1280×720 instead of 1920×1080"
		);
		assert_eq!(
			messages[2],
			"frame rate differs (Show S01E02.mkv): 25 instead of 23.976"
		);
		assert_eq!(
			messages[5],
			"sample rate differs (Show S01E02.mkv): 44.1 kHz instead of 48 kHz"
		);
	}

	#[test]
	fn equal_frame_rates_in_other_fractions_match() {
		let mut other = standard();
		other[0] = video(1920, 1080, (2997, 125));
		let parts = [episode(1, 10, standard()), episode(2, 10, other)];
		assert!(check(&parts, &[]).unwrap().can_merge());
	}

	#[test]
	fn unknown_values_do_not_count_as_different() {
		let mut other = standard();
		other[0].pix_fmt = None;
		other[1].sample_rate = None;
		let parts = [episode(1, 10, standard()), episode(2, 10, other)];
		assert!(check(&parts, &[]).unwrap().can_merge());
	}

	#[test]
	fn a_different_stream_layout_blocks_the_merge() {
		let mut fewer = standard();
		fewer.remove(2);
		let mut swapped = standard();
		swapped.swap(0, 1);
		let parts = [
			episode(1, 10, standard()),
			episode(2, 10, fewer),
			episode(3, 10, swapped),
		];
		let check = check(&parts, &[]).unwrap();
		assert_eq!(check.layout.len(), 2);
		assert_eq!(
			check.layout[0].to_string(),
			"stream layout differs (Show S01E02.mkv): video, audio, subtitle instead of video, audio, audio, subtitle"
		);
		assert_eq!(check.layout[0].stream, 2);
		assert_eq!(check.layout[1].stream, 0);
		// The table still shows what each part has at each index.
		assert_eq!(check.rows[3].cells[1], None);
		assert_eq!(
			check.rows[0].cells[2].as_deref(),
			Some("ac3 5.1 48 kHz eng")
		);
		// Streams of another kind at the same index are not compared.
		assert!(check.rows[0].identical());
		// Dropping does not help a layout mismatch.
		assert!(!super::check(&parts, &[0, 1, 2]).unwrap().can_merge());
	}

	#[test]
	fn requests_that_are_not_a_merge_are_errors() {
		let one = [episode(1, 10, standard())];
		assert_eq!(check(&one, &[]), Err(CheckError::TooFewParts));
		assert_eq!(check(&[], &[]), Err(CheckError::TooFewParts));
		let two = [episode(1, 10, standard()), episode(2, 10, standard())];
		assert_eq!(check(&two, &[7]), Err(CheckError::UnknownStream(7)));
		assert_eq!(check(&two, &[0, 1, 2, 3]), Err(CheckError::NothingLeft));
		assert_eq!(
			CheckError::TooFewParts.to_string(),
			"a merge needs at least 2 files"
		);
	}

	#[test]
	fn the_total_duration_needs_every_part() {
		let mut unknown = episode(2, 10, standard());
		unknown.duration = None;
		let parts = [episode(1, 10, standard()), unknown];
		assert_eq!(check(&parts, &[]).unwrap().total_duration, None);
	}

	#[test]
	fn summaries_fall_back_when_values_are_missing() {
		let bare = StreamParams {
			index: 4,
			kind: CodecType::Audio,
			channels: Some(2),
			..StreamParams::default()
		};
		assert_eq!(bare.summary(), "audio 2 ch");
		let data = StreamParams {
			index: 5,
			kind: CodecType::Data,
			codec: Some("bin_data".into()),
			..StreamParams::default()
		};
		assert_eq!(data.summary(), "bin_data");
		assert_eq!(fps(25.0), "25");
		assert_eq!(fps(29.97), "29.97");
		assert_eq!(khz(44_100), "44.1 kHz");
		assert_eq!(khz(22_050), "22.05 kHz");
	}

	#[test]
	fn a_part_comes_from_a_probe() {
		let probe = crate::ffmpeg::probe::parse(
			r#"{"format":{"duration":"12.5"},"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264","width":1280,"height":720,"avg_frame_rate":"0/0","r_frame_rate":"30/1","pix_fmt":"yuv420p"},
				{"index":1,"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":2,"channel_layout":"stereo","tags":{"language":"eng"}}
			]}"#,
		)
		.unwrap();
		let part = Part::from_probe(Path::new("/media/in/clip.mp4"), &probe);
		let und = crate::ffmpeg::probe::parse(
			r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","tags":{"language":"und"}}]}"#,
		)
		.unwrap();
		assert_eq!(
			Part::from_probe(Path::new("/x.mp4"), &und).streams[0].summary(),
			"h264"
		);
		assert_eq!(part.name, "clip.mp4");
		assert_eq!(part.duration, Some(Duration::from_millis(12_500)));
		assert_eq!(part.layout(), "video, audio");
		assert_eq!(part.streams[0].summary(), "h264 1280×720 30");
		assert_eq!(part.streams[1].summary(), "aac stereo 48 kHz eng");
	}
}
