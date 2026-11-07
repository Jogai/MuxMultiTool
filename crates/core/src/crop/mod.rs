//! Lossless crop: hiding black bars without re-encoding.
//!
//! H.264 and H.265 streams carry a crop window in their sequence header
//! (the SPS `frame_cropping` offsets, the `conformance_window` in HEVC).
//! Decoders show only the window, so black bars disappear when the window
//! is made smaller. ffmpeg's `h264_metadata` and `hevc_metadata` bitstream
//! filters rewrite that window on a `-c copy` run ([`command::crop`]), so
//! the pixels are untouched: the file keeps its size, the quality stays,
//! and the job takes as long as a remux. No other codec stores a crop
//! window, so every other codec is refused ([`Unsupported`]).
//!
//! The offsets are stored in chroma units, which is why a crop must be a
//! multiple of the [`CropUnit`]: 2 px for 4:2:0 video, the common case.
//! [`Crop::snap`] rounds a request to the unit, and the frontend shows the
//! snapped values. The header's offsets are relative to the coded picture,
//! which encoders pad to a multiple of their block size (1080p is coded as
//! 1088 lines with 8 cropped off), and the bitstream filters replace the
//! offsets instead of adding to them. [`Coded`] holds what the header has
//! now, read with one decoded frame ([`detect::coded_probe_args`]), and
//! [`Plan::header_crop`] adds the request to it.
//!
//! Black bars are found with ffmpeg's `cropdetect` filter over a handful of
//! keyframes ([`command::detect`], [`detect::parse_cropdetect`]).
//!
//! [`limitations`] is the honest note the frontend always shows: the file
//! size stays the same, and some players ignore the crop window.

use std::time::Duration;

use crate::ffmpeg::Capabilities;
use crate::ffmpeg::probe::{CodecType, ProbeResult};

pub mod command;
pub mod detect;

/// How many keyframes black-bar detection looks at.
pub const DETECT_FRAMES: u32 = 12;

/// The least picture a crop must leave in each direction, in pixels: one
/// macroblock.
pub const MIN_SIZE: u32 = 16;

/// The filter black-bar detection needs from the ffmpeg build.
const DETECT_FILTER: &str = "cropdetect";

/// A codec whose stream header has a crop window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
	/// H.264 / AVC.
	H264,
	/// H.265 / HEVC.
	Hevc,
}

impl Codec {
	/// The codec for ffprobe's `codec_name`, or `None` for every other
	/// codec.
	#[must_use]
	pub fn from_name(name: &str) -> Option<Self> {
		match name {
			"h264" => Some(Self::H264),
			"hevc" => Some(Self::Hevc),
			_ => None,
		}
	}

	/// The name as the user knows it.
	#[must_use]
	pub const fn name(self) -> &'static str {
		match self {
			Self::H264 => "H.264",
			Self::Hevc => "H.265",
		}
	}

	/// The bitstream filter that rewrites this codec's crop window.
	#[must_use]
	pub const fn bitstream_filter(self) -> &'static str {
		match self {
			Self::H264 => "h264_metadata",
			Self::Hevc => "hevc_metadata",
		}
	}
}

/// The granularity of the crop window: the header stores offsets in chroma
/// samples, so a crop must be a multiple of the chroma subsampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropUnit {
	/// Horizontal unit in pixels: 2 for 4:2:0 and 4:2:2, 1 for 4:4:4.
	pub x: u32,
	/// Vertical unit in pixels: 2 for 4:2:0, 1 for 4:2:2 and 4:4:4; twice
	/// that for interlaced H.264, whose offsets count field lines.
	pub y: u32,
}

impl CropUnit {
	/// The unit for a stream, or `None` when the pixel format's chroma
	/// subsampling is unknown.
	#[must_use]
	pub fn for_stream(codec: Codec, pix_fmt: &str, interlaced: bool) -> Option<Self> {
		let (x, y) = chroma_subsampling(pix_fmt)?;
		let y = if codec == Codec::H264 && interlaced {
			y * 2
		} else {
			y
		};
		Some(Self { x, y })
	}
}

/// The chroma subsampling of a pixel format as `(horizontal, vertical)`
/// factors, read from the format name: `yuv420p10le` is 4:2:0, `nv12` is
/// 4:2:0, `gray` has no chroma. `None` for a name this does not know.
fn chroma_subsampling(pix_fmt: &str) -> Option<(u32, u32)> {
	if pix_fmt.contains("420")
		|| pix_fmt.starts_with("nv12")
		|| pix_fmt.starts_with("nv21")
		|| pix_fmt.starts_with("p01")
	{
		Some((2, 2))
	} else if pix_fmt.contains("422") || pix_fmt.starts_with("nv16") || pix_fmt.starts_with("p21") {
		Some((2, 1))
	} else if pix_fmt.contains("444")
		|| pix_fmt.starts_with("gray")
		|| pix_fmt.starts_with("nv24")
		|| pix_fmt.starts_with("nv42")
		|| pix_fmt.starts_with("p41")
	{
		Some((1, 1))
	} else {
		None
	}
}

/// How much to cut off each edge, in pixels of the picture as it is shown
/// now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Crop {
	/// Lines off the top.
	pub top: u32,
	/// Lines off the bottom.
	pub bottom: u32,
	/// Columns off the left.
	pub left: u32,
	/// Columns off the right.
	pub right: u32,
}

impl Crop {
	/// Every edge rounded to the nearest multiple of the unit, halves
	/// rounded down so a snap never cuts more picture than asked.
	#[must_use]
	pub const fn snap(self, unit: CropUnit) -> Self {
		Self {
			top: snap(self.top, unit.y),
			bottom: snap(self.bottom, unit.y),
			left: snap(self.left, unit.x),
			right: snap(self.right, unit.x),
		}
	}

	/// `true` when nothing is cut off.
	#[must_use]
	pub const fn is_empty(self) -> bool {
		self.top == 0 && self.bottom == 0 && self.left == 0 && self.right == 0
	}

	/// Whether every edge is a multiple of the unit, which the bitstream
	/// filters require.
	#[must_use]
	pub const fn is_snapped(self, unit: CropUnit) -> bool {
		self.top.is_multiple_of(unit.y)
			&& self.bottom.is_multiple_of(unit.y)
			&& self.left.is_multiple_of(unit.x)
			&& self.right.is_multiple_of(unit.x)
	}

	/// This crop on top of `other`: each edge added.
	#[must_use]
	pub const fn plus(self, other: Self) -> Self {
		Self {
			top: self.top + other.top,
			bottom: self.bottom + other.bottom,
			left: self.left + other.left,
			right: self.right + other.right,
		}
	}
}

/// `value` rounded to the nearest multiple of `unit`, halves down.
const fn snap(value: u32, unit: u32) -> u32 {
	if unit <= 1 {
		return value;
	}
	let rem = value % unit;
	if rem * 2 > unit {
		value + unit - rem
	} else {
		value - rem
	}
}

/// The video stream a crop applies to: the first real video stream of the
/// file (cover art does not count).
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
	/// The stream's position in the container, for `-map 0:<index>`.
	pub stream_index: u32,
	/// The stream's position among the video streams, for `-bsf:v:<n>`.
	pub video_ordinal: u32,
	/// The codec, one with a crop window.
	pub codec: Codec,
	/// The picture width as shown now.
	pub width: u32,
	/// The picture height as shown now.
	pub height: u32,
	/// The crop unit of this stream.
	pub unit: CropUnit,
	/// The pixel format, e.g. `yuv420p`, for the "snaps to 2 px (4:2:0)"
	/// note.
	pub pix_fmt: String,
	/// The duration, when the probe knows it: the job's expected duration.
	pub duration: Option<Duration>,
}

/// Why a file cannot be cropped losslessly.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unsupported {
	/// The file has no video stream with a picture size.
	#[error("the file has no video stream")]
	NoVideo,
	/// The video is not H.264 or H.265.
	#[error("lossless crop needs H.264 or H.265; this video is {name}")]
	Codec {
		/// The codec as ffprobe names it, e.g. `vp9`.
		name: String,
	},
	/// The pixel format's chroma subsampling, and so the crop unit, is
	/// unknown.
	#[error("lossless crop does not know the crop unit of pixel format {0}")]
	PixelFormat(String),
	/// The build lacks the bitstream filter or the detection filter.
	#[error("this ffmpeg build lacks {0}")]
	Build(String),
}

impl Source {
	/// The first video stream of `probe` that is not an attached picture.
	///
	/// # Errors
	/// [`Unsupported`] when there is no such stream, its codec has no crop
	/// window, or its crop unit is unknown.
	pub fn from_probe(probe: &ProbeResult) -> Result<Self, Unsupported> {
		let (ordinal, video) = probe
			.streams_of(CodecType::Video)
			.enumerate()
			.find(|(_, s)| !s.disposition.attached_pic)
			.ok_or(Unsupported::NoVideo)?;
		let (Some(width), Some(height)) = (video.width, video.height) else {
			return Err(Unsupported::NoVideo);
		};
		if width == 0 || height == 0 {
			return Err(Unsupported::NoVideo);
		}
		let name = video.codec_name.clone().unwrap_or_default();
		let codec = Codec::from_name(&name).ok_or_else(|| Unsupported::Codec {
			name: if name.is_empty() {
				"unknown".to_owned()
			} else {
				name.clone()
			},
		})?;
		let pix_fmt = video.pix_fmt.clone().unwrap_or_default();
		let interlaced = video
			.field_order
			.as_deref()
			.is_some_and(|order| !matches!(order, "progressive" | "unknown"));
		let unit = CropUnit::for_stream(codec, &pix_fmt, interlaced)
			.ok_or_else(|| Unsupported::PixelFormat(pix_fmt.clone()))?;
		Ok(Self {
			stream_index: video.index,
			video_ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
			codec,
			width,
			height,
			unit,
			pix_fmt,
			duration: probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
		})
	}

	/// What the build must have to crop this stream and to detect its black
	/// bars, and lacks.
	///
	/// # Errors
	/// [`Unsupported::Build`] naming the missing bitstream filter or filter.
	pub fn check_build(&self, caps: &Capabilities) -> Result<(), Unsupported> {
		let bsf = self.codec.bitstream_filter();
		if !caps.has_bitstream_filter(bsf) {
			return Err(Unsupported::Build(format!("the {bsf} bitstream filter")));
		}
		if !caps.has_filter(DETECT_FILTER) {
			return Err(Unsupported::Build(format!("the {DETECT_FILTER} filter")));
		}
		Ok(())
	}
}

/// The picture as the stream stores it: the coded size and the crop window
/// the header has now. ffprobe shows it when told not to apply the crop,
/// see [`detect::coded_probe_args`] and [`detect::parse_coded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coded {
	/// The coded width, a multiple of the encoder's block size.
	pub width: u32,
	/// The coded height.
	pub height: u32,
	/// The crop window in the header now; usually only the encoder's
	/// padding at the right and bottom.
	pub crop: Crop,
}

/// Why a crop cannot be planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
	/// Left plus right leaves less than [`MIN_SIZE`] columns.
	#[error("cropping {cut} px from a {width} px wide picture leaves less than {MIN_SIZE} px")]
	TooNarrow {
		/// Left plus right, snapped.
		cut: u32,
		/// The picture width.
		width: u32,
	},
	/// Top plus bottom leaves less than [`MIN_SIZE`] lines.
	#[error("cropping {cut} px from a {height} px high picture leaves less than {MIN_SIZE} px")]
	TooShort {
		/// Top plus bottom, snapped.
		cut: u32,
		/// The picture height.
		height: u32,
	},
}

/// A crop resolved against its stream: the snapped values and the size
/// they leave.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
	/// The stream.
	pub source: Source,
	/// What was asked for.
	pub requested: Crop,
	/// The request snapped to the stream's crop unit.
	pub crop: Crop,
	/// The width the picture will show after the crop.
	pub width: u32,
	/// The height the picture will show after the crop.
	pub height: u32,
}

/// Snaps `requested` to the unit of `source` and checks what it leaves.
///
/// An empty crop is a valid plan (the frontend asks for one while the user
/// types); the job submit refuses it, see [`Plan::is_empty`].
///
/// # Errors
/// [`PlanError`] when the crop leaves less than [`MIN_SIZE`] px either way.
pub fn plan(source: &Source, requested: Crop) -> Result<Plan, PlanError> {
	let crop = requested.snap(source.unit);
	let cut_x = crop.left.saturating_add(crop.right);
	let cut_y = crop.top.saturating_add(crop.bottom);
	if cut_x.saturating_add(MIN_SIZE) > source.width {
		return Err(PlanError::TooNarrow {
			cut: cut_x,
			width: source.width,
		});
	}
	if cut_y.saturating_add(MIN_SIZE) > source.height {
		return Err(PlanError::TooShort {
			cut: cut_y,
			height: source.height,
		});
	}
	Ok(Plan {
		source: source.clone(),
		requested,
		crop,
		width: source.width - cut_x,
		height: source.height - cut_y,
	})
}

impl Plan {
	/// `true` when the crop cuts nothing, so a job would change nothing.
	#[must_use]
	pub const fn is_empty(&self) -> bool {
		self.crop.is_empty()
	}

	/// The crop window to write into the header: the window it has now
	/// plus this plan's crop, since the bitstream filters replace the
	/// offsets rather than add to them.
	#[must_use]
	pub const fn header_crop(&self, coded: &Coded) -> Crop {
		coded.crop.plus(self.crop)
	}
}

/// What the frontend tells the user before a crop, always visible (see the
/// GUI sketch, "Honest limitations").
#[must_use]
pub fn limitations(codec: Codec) -> Vec<String> {
	vec![
		format!(
			"Only the crop window in the {} header changes; nothing is re-encoded.",
			codec.name()
		),
		"The file size stays the same: the cropped pixels are still stored, only hidden."
			.to_owned(),
		"Some players and editors ignore the crop window and show the full picture.".to_owned(),
	]
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe::{self, Stream};

	const MP4: &str = include_str!("../../tests/fixtures/ffprobe/mp4.json");
	const MKV: &str = include_str!("../../tests/fixtures/ffprobe/mkv.json");

	const UNIT_420: CropUnit = CropUnit { x: 2, y: 2 };

	fn source() -> Source {
		Source {
			stream_index: 0,
			video_ordinal: 0,
			codec: Codec::Hevc,
			width: 3840,
			height: 2160,
			unit: UNIT_420,
			pix_fmt: "yuv420p10le".into(),
			duration: Some(Duration::from_secs(5400)),
		}
	}

	#[test]
	fn only_h264_and_hevc_have_a_crop_window() {
		assert_eq!(Codec::from_name("h264"), Some(Codec::H264));
		assert_eq!(Codec::from_name("hevc"), Some(Codec::Hevc));
		for other in ["vp9", "av1", "mpeg4", "mpeg2video", "prores", ""] {
			assert_eq!(Codec::from_name(other), None, "{other}");
		}
		assert_eq!(Codec::H264.bitstream_filter(), "h264_metadata");
		assert_eq!(Codec::Hevc.bitstream_filter(), "hevc_metadata");
		assert_eq!(Codec::Hevc.name(), "H.265");
	}

	#[test]
	fn crop_unit_follows_the_chroma_subsampling() {
		let unit = |fmt: &str| CropUnit::for_stream(Codec::Hevc, fmt, false);
		for fmt in ["yuv420p", "yuvj420p", "yuv420p10le", "nv12", "p010le"] {
			assert_eq!(unit(fmt), Some(UNIT_420), "{fmt}");
		}
		for fmt in ["yuv422p", "yuv422p10le", "nv16"] {
			assert_eq!(unit(fmt), Some(CropUnit { x: 2, y: 1 }), "{fmt}");
		}
		for fmt in ["yuv444p", "yuv444p12le", "gray", "gray10le"] {
			assert_eq!(unit(fmt), Some(CropUnit { x: 1, y: 1 }), "{fmt}");
		}
		assert_eq!(unit("rgb24"), None);
		assert_eq!(unit(""), None);
	}

	#[test]
	fn interlaced_h264_counts_field_lines() {
		assert_eq!(
			CropUnit::for_stream(Codec::H264, "yuv420p", true),
			Some(CropUnit { x: 2, y: 4 })
		);
		// HEVC has no field coding in the header
		assert_eq!(
			CropUnit::for_stream(Codec::Hevc, "yuv420p", true),
			Some(UNIT_420)
		);
	}

	#[test]
	fn snapping_rounds_to_the_nearest_unit_halves_down() {
		let crop = Crop {
			top: 141,
			bottom: 139,
			left: 1,
			right: 0,
		};
		assert_eq!(
			crop.snap(UNIT_420),
			Crop {
				top: 140,
				bottom: 138,
				left: 0,
				right: 0,
			}
		);
		assert_eq!(
			Crop {
				top: 141,
				bottom: 143,
				left: 2,
				right: 0,
			}
			.snap(CropUnit { x: 2, y: 4 }),
			Crop {
				top: 140,
				bottom: 144,
				left: 2,
				right: 0,
			}
		);
		assert_eq!(crop.snap(CropUnit { x: 1, y: 1 }), crop);
		assert!(crop.snap(UNIT_420).is_snapped(UNIT_420));
		assert!(!crop.is_snapped(UNIT_420));
		assert!(Crop::default().is_empty());
		assert!(!crop.is_empty());
	}

	#[test]
	fn a_plan_snaps_and_sizes_the_result() {
		let plan = plan(
			&source(),
			Crop {
				top: 141,
				bottom: 140,
				left: 0,
				right: 0,
			},
		)
		.unwrap();
		assert_eq!(plan.crop.top, 140);
		assert_eq!(plan.requested.top, 141);
		assert_eq!((plan.width, plan.height), (3840, 1880));
		assert!(!plan.is_empty());
		let coded = Coded {
			width: 3840,
			height: 2176,
			crop: Crop {
				bottom: 16,
				..Crop::default()
			},
		};
		assert_eq!(
			plan.header_crop(&coded),
			Crop {
				top: 140,
				bottom: 156,
				left: 0,
				right: 0,
			}
		);
		assert_eq!(
			coded.height - plan.header_crop(&coded).top - plan.header_crop(&coded).bottom,
			plan.height
		);
	}

	#[test]
	fn an_empty_plan_is_allowed_but_marked() {
		let plan = plan(&source(), Crop::default()).unwrap();
		assert!(plan.is_empty());
		assert_eq!((plan.width, plan.height), (3840, 2160));
	}

	#[test]
	fn a_crop_must_leave_a_macroblock() {
		let narrow = plan(
			&source(),
			Crop {
				left: 3000,
				right: 830,
				..Crop::default()
			},
		)
		.unwrap_err();
		assert_eq!(
			narrow,
			PlanError::TooNarrow {
				cut: 3830,
				width: 3840
			}
		);
		assert_eq!(
			narrow.to_string(),
			"cropping 3830 px from a 3840 px wide picture leaves less than 16 px"
		);
		let short = plan(
			&source(),
			Crop {
				top: 2160,
				..Crop::default()
			},
		)
		.unwrap_err();
		assert!(matches!(short, PlanError::TooShort { cut: 2160, .. }));
		assert!(
			plan(
				&source(),
				Crop {
					left: 3824,
					..Crop::default()
				}
			)
			.is_ok()
		);
	}

	#[test]
	fn source_comes_from_the_first_real_video_stream() {
		let probe = probe::parse(MP4).unwrap();
		let source = Source::from_probe(&probe).unwrap();
		assert_eq!(source.codec, Codec::H264);
		assert_eq!(source.stream_index, 0);
		assert_eq!(source.video_ordinal, 0);
		assert!(source.width > 0 && source.height > 0);
		assert_eq!(source.unit, UNIT_420);
		assert!(source.duration.is_some());

		let probe = probe::parse(MKV).unwrap();
		let source = Source::from_probe(&probe);
		// The MKV fixture's codec decides: either way the answer is explicit.
		match source {
			Ok(source) => assert!(matches!(source.codec, Codec::H264 | Codec::Hevc)),
			Err(error) => assert!(matches!(error, Unsupported::Codec { .. }), "{error}"),
		}
	}

	#[test]
	fn cover_art_is_skipped_and_other_codecs_are_refused() {
		let mut probe = probe::parse(MP4).unwrap();
		let mut cover = Stream {
			index: 9,
			codec_type: CodecType::Video,
			codec_name: Some("mjpeg".into()),
			width: Some(600),
			height: Some(600),
			pix_fmt: Some("yuvj420p".into()),
			..Stream::default()
		};
		cover.disposition.attached_pic = true;
		probe.streams.insert(0, cover);
		let source = Source::from_probe(&probe).unwrap();
		assert_eq!(source.codec, Codec::H264);
		assert_eq!(source.video_ordinal, 1);

		probe.streams[1].codec_name = Some("vp9".into());
		let error = Source::from_probe(&probe).unwrap_err();
		assert_eq!(error, Unsupported::Codec { name: "vp9".into() });
		assert_eq!(
			error.to_string(),
			"lossless crop needs H.264 or H.265; this video is vp9"
		);

		probe.streams[1].codec_name = Some("hevc".into());
		probe.streams[1].pix_fmt = Some("rgb24".into());
		assert_eq!(
			Source::from_probe(&probe).unwrap_err(),
			Unsupported::PixelFormat("rgb24".into())
		);

		probe.streams.retain(|s| s.codec_type != CodecType::Video);
		assert_eq!(
			Source::from_probe(&probe).unwrap_err(),
			Unsupported::NoVideo
		);
	}

	#[test]
	fn the_build_needs_the_bitstream_filter_and_cropdetect() {
		let caps = |bsfs: &[&str], filters: &[&str]| Capabilities {
			version: "test".into(),
			encoders: Vec::new(),
			decoders: Vec::new(),
			formats: Vec::new(),
			filters: filters
				.iter()
				.map(|name| crate::ffmpeg::capabilities::Filter {
					name: (*name).to_owned(),
					description: String::new(),
					timeline: true,
				})
				.collect(),
			configuration: Vec::new(),
			hwaccels: Vec::new(),
			bitstream_filters: bsfs.iter().map(|b| (*b).to_owned()).collect(),
		};
		let source = source();
		assert_eq!(
			source.check_build(&caps(&["hevc_metadata"], &["cropdetect"])),
			Ok(())
		);
		assert_eq!(
			source.check_build(&caps(&["h264_metadata"], &["cropdetect"])),
			Err(Unsupported::Build(
				"the hevc_metadata bitstream filter".into()
			))
		);
		assert_eq!(
			source
				.check_build(&caps(&["hevc_metadata"], &[]))
				.unwrap_err()
				.to_string(),
			"this ffmpeg build lacks the cropdetect filter"
		);
	}

	#[test]
	fn limitations_name_the_codec_and_the_two_surprises() {
		let notes = limitations(Codec::H264);
		assert_eq!(notes.len(), 3);
		assert!(notes[0].contains("H.264 header"));
		assert!(notes[1].contains("file size stays the same"));
		assert!(notes[2].contains("ignore the crop window"));
	}
}
