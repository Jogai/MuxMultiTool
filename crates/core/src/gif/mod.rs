//! Animated GIFs: a range of a video to a GIF within limits, and a GIF back
//! to a video.
//!
//! GIF files grow fast: every frame is stored as indexed pixels with LZW
//! compression, so a few seconds at a modest size reach megabytes. Instead
//! of exposing ffmpeg's knobs, the user states the frame rate and width and
//! what the GIF must stay within ([`Limits`]); [`plan`] works out the
//! picture size, the frame count and an estimated file size, and
//! [`Plan::violations`] names every limit the result would break, with a
//! suggestion each. A job is only submitted for a plan without violations,
//! so the user sees the problem before ffmpeg runs, not a huge file after.
//!
//! The GIF is made in one ffmpeg pass with a palette computed from the
//! range (`palettegen` + `paletteuse`), see [`command`]. The reverse,
//! [`command::gif_to_video`], re-encodes a GIF to H.264 in MP4 or VP9/VP8
//! in `WebM` with whichever encoder the build has ([`VideoFormat::encoder`]).

use std::fmt;
use std::path::Path;
use std::time::Duration;

use crate::ffmpeg::probe::{CodecType, ProbeResult};
use crate::ffmpeg::{Capabilities, FfmpegCommand};

pub mod command;

/// The frame rate used when the request leaves it at 0.
pub const DEFAULT_FPS: u32 = 12;
/// The width used when the request leaves it at 0.
pub const DEFAULT_WIDTH: u32 = 480;
/// The highest frame rate a GIF can express: frame delays are in 1/100 s
/// and players treat delays under 2/100 s as 10/100 s.
pub const MAX_FPS: u32 = 50;
/// The narrowest GIF a request may ask for.
pub const MIN_WIDTH: u32 = 16;
/// The widest GIF a request may ask for, whatever the limits say.
pub const MAX_WIDTH: u32 = 4096;

/// One megabyte as the limits and messages count it: decimal, like file
/// managers and the GUI sketch.
pub const MB: u64 = 1_000_000;

/// The encoder and filters a GIF job needs from the ffmpeg build.
const GIF_ENCODER: &str = "gif";
const GIF_FILTERS: [&str; 5] = ["fps", "scale", "split", "palettegen", "paletteuse"];

/// The video stream a GIF is made from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
	/// Picture width in pixels.
	pub width: u32,
	/// Picture height in pixels.
	pub height: u32,
	/// Frame rate, when the probe knows it.
	pub frame_rate: Option<f64>,
	/// Duration of the input, when the probe knows it.
	pub duration: Option<Duration>,
}

impl Source {
	/// The first video stream of `probe` with a known picture size, or
	/// `None` when the input has no usable video.
	#[must_use]
	pub fn from_probe(probe: &ProbeResult) -> Option<Self> {
		let video = probe
			.streams_of(CodecType::Video)
			.find(|s| s.width.is_some_and(|w| w > 0) && s.height.is_some_and(|h| h > 0))?;
		Some(Self {
			width: video.width?,
			height: video.height?,
			frame_rate: video.frame_rate().filter(|f| f.is_finite() && *f > 0.0),
			duration: probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
		})
	}
}

/// How much picture quality to trade for file size: the palette's colors
/// and the dither. More colors and finer dither look better and compress
/// worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
	/// 64 colors and a coarse ordered dither: the smallest file.
	Small,
	/// 128 colors and a fine ordered dither.
	#[default]
	Balanced,
	/// 256 colors and error-diffusion dither: the best picture.
	Best,
}

impl Quality {
	/// Colors in the palette, `palettegen`'s `max_colors`.
	#[must_use]
	pub const fn max_colors(self) -> u32 {
		match self {
			Self::Small => 64,
			Self::Balanced => 128,
			Self::Best => 256,
		}
	}

	/// The `paletteuse` dither options.
	#[must_use]
	pub const fn dither(self) -> &'static str {
		match self {
			Self::Small => "bayer:bayer_scale=5",
			Self::Balanced => "bayer:bayer_scale=3",
			Self::Best => "sierra2_4a",
		}
	}

	/// How many bytes one pixel costs after LZW, on average for typical
	/// video. Measured on real clips, these are rough: dither adds noise
	/// that compresses badly, and a static scene compresses far better
	/// than motion, so the estimate leans to the high side.
	#[must_use]
	pub const fn bytes_per_pixel(self) -> f64 {
		match self {
			Self::Small => 0.3,
			Self::Balanced => 0.45,
			Self::Best => 0.6,
		}
	}
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
	/// Where the range starts, from the start of the input.
	pub start: Duration,
	/// Where the range ends; it is clipped to the input's duration.
	pub end: Duration,
	/// Frames per second, 1 to [`MAX_FPS`]; 0 for [`DEFAULT_FPS`]. The GIF
	/// never runs faster than the source.
	pub fps: u32,
	/// The GIF's width, [`MIN_WIDTH`] to [`MAX_WIDTH`]; 0 for
	/// [`DEFAULT_WIDTH`]. The GIF is never wider than the source.
	pub width: u32,
	/// See [`Quality`].
	pub quality: Quality,
	/// Play once instead of looping forever.
	pub play_once: bool,
}

/// What a GIF must stay within. The user picks these; see the GUI sketch's
/// "Limits, not settings".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
	/// The longest range.
	pub max_duration: Duration,
	/// The widest picture.
	pub max_width: u32,
	/// The tallest picture, or `None` for no bound.
	pub max_height: Option<u32>,
	/// The largest estimated file.
	pub max_bytes: u64,
}

impl Default for Limits {
	/// 10 s, 480 px wide, any height, 8 MB: the sketch's defaults.
	fn default() -> Self {
		Self {
			max_duration: Duration::from_secs(10),
			max_width: 480,
			max_height: None,
			max_bytes: 8 * MB,
		}
	}
}

/// Why a request cannot be planned at all, as opposed to breaking a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
	/// The end is not after the start.
	#[error("the range must end after it starts")]
	EmptyRange,
	/// The start is at or past the end of the input.
	#[error("the range starts after the end of the input")]
	StartPastEnd,
	/// The frame rate is outside 1 to [`MAX_FPS`].
	#[error("the frame rate must be between 1 and {MAX_FPS}")]
	FrameRate,
	/// The width is outside [`MIN_WIDTH`] to [`MAX_WIDTH`].
	#[error("the width must be between {MIN_WIDTH} and {MAX_WIDTH} px")]
	Width,
}

/// Which limit a [`Violation`] breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
	/// [`Limits::max_duration`].
	Duration,
	/// [`Limits::max_width`].
	Width,
	/// [`Limits::max_height`].
	Height,
	/// [`Limits::max_bytes`].
	Size,
}

/// One limit a plan breaks, with what to lower.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
	/// The limit.
	pub limit: Limit,
	/// The problem and the suggestion in one sentence, for the user.
	pub message: String,
	/// A range length that fits, when shortening helps.
	pub suggested_duration: Option<Duration>,
	/// A width that fits, when narrowing helps.
	pub suggested_width: Option<u32>,
}

impl fmt::Display for Violation {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.message)
	}
}

/// A request resolved against its source: what the GIF will be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
	/// See [`Request::start`].
	pub start: Duration,
	/// The length of the GIF: the range, cut at the end of the input.
	pub duration: Duration,
	/// The frame rate, the request's unless the source is slower.
	pub fps: u32,
	/// The width, the request's unless the source is narrower.
	pub width: u32,
	/// The height, from the width and the source's aspect ratio.
	pub height: u32,
	/// How many frames the GIF holds.
	pub frames: u32,
	/// The estimated file size; see [`estimate_bytes`].
	pub estimated_bytes: u64,
	/// See [`Request::quality`].
	pub quality: Quality,
	/// See [`Request::play_once`].
	pub play_once: bool,
}

/// Resolves `request` against `source`.
///
/// # Errors
/// See [`PlanError`]: the request is contradictory or out of the range any
/// GIF supports. Breaking a limit is not an error here; see
/// [`Plan::violations`].
pub fn plan(request: &Request, source: &Source) -> Result<Plan, PlanError> {
	if request.end <= request.start {
		return Err(PlanError::EmptyRange);
	}
	let fps = match request.fps {
		0 => DEFAULT_FPS,
		fps @ 1..=MAX_FPS => fps,
		_ => return Err(PlanError::FrameRate),
	};
	let width = match request.width {
		0 => DEFAULT_WIDTH,
		w @ MIN_WIDTH..=MAX_WIDTH => w,
		_ => return Err(PlanError::Width),
	};
	let end = match source.duration {
		Some(total) if request.start >= total => return Err(PlanError::StartPastEnd),
		Some(total) => request.end.min(total),
		None => request.end,
	};
	let duration = end.saturating_sub(request.start);
	// Never faster than the source: `fps` would only duplicate frames.
	let fps = source
		.frame_rate
		.map_or(fps, |rate| fps.min(whole_fps(rate)));
	// Never wider than the source: upscaling only adds bytes.
	let width = width.min(source.width).max(1);
	let height = scaled_height(width, source);
	let frames = frames_in(duration, fps);
	Ok(Plan {
		start: request.start,
		duration,
		fps,
		width,
		height,
		frames,
		estimated_bytes: estimate_bytes(frames, width, height, request.quality),
		quality: request.quality,
		play_once: request.play_once,
	})
}

/// A frame rate rounded up to whole frames, within 1 to [`MAX_FPS`].
#[expect(
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss,
	reason = "clamped to 1..=MAX_FPS first"
)]
fn whole_fps(rate: f64) -> u32 {
	rate.ceil().clamp(1.0, f64::from(MAX_FPS)) as u32
}

/// The height that keeps the source's aspect ratio at `width`, at least 1.
#[expect(
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss,
	reason = "a rounded ratio of small positive integers"
)]
fn scaled_height(width: u32, source: &Source) -> u32 {
	let height = f64::from(width) * f64::from(source.height) / f64::from(source.width);
	(height.round() as u32).max(1)
}

/// `width` scaled by `plan_height / plan_width`, in integers, at least 1:
/// the height a plan would have at another width, or the other way round.
fn height_at(width: u32, plan_width: u32, plan_height: u32) -> u32 {
	let height = u64::from(width) * u64::from(plan_height) / u64::from(plan_width);
	u32::try_from(height).unwrap_or(u32::MAX).max(1)
}

/// How many frames the `fps` filter emits for `duration`: one per period,
/// the first at 0, so a range of exactly `n` periods gives `n` frames.
fn frames_in(duration: Duration, fps: u32) -> u32 {
	let periods = duration.as_millis() * u128::from(fps);
	u32::try_from(periods.div_ceil(1000)).unwrap_or(u32::MAX)
}

/// The estimated size of a GIF of `frames` frames of `width` × `height`
/// pixels at `quality`: the headers and palette, then each frame's header
/// and its pixels at [`Quality::bytes_per_pixel`].
#[must_use]
#[expect(
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss,
	reason = "a positive byte count that fits long before the file would"
)]
pub fn estimate_bytes(frames: u32, width: u32, height: u32, quality: Quality) -> u64 {
	// Header, logical screen descriptor, a 256-entry palette and the
	// Netscape loop extension; then an image descriptor and graphic control
	// extension per frame.
	const FILE_OVERHEAD: f64 = 13.0 + 768.0 + 19.0;
	const FRAME_OVERHEAD: f64 = 18.0;
	let pixels = f64::from(width) * f64::from(height);
	let per_frame = FRAME_OVERHEAD + pixels * quality.bytes_per_pixel();
	(FILE_OVERHEAD + f64::from(frames) * per_frame).round() as u64
}

impl Plan {
	/// Whether the plan breaks none of `limits`.
	#[must_use]
	pub fn within(&self, limits: &Limits) -> bool {
		self.violations(limits).is_empty()
	}

	/// Every limit the plan breaks, in the order of [`Limit`], each with a
	/// suggestion that would fit that one limit.
	#[must_use]
	pub fn violations(&self, limits: &Limits) -> Vec<Violation> {
		let mut found = Vec::new();
		if self.duration > limits.max_duration {
			found.push(Violation {
				limit: Limit::Duration,
				message: format!(
					"the range is {} long, over the {} limit; shorten it to {}",
					secs(self.duration),
					secs(limits.max_duration),
					secs(limits.max_duration)
				),
				suggested_duration: Some(limits.max_duration),
				suggested_width: None,
			});
		}
		if self.width > limits.max_width {
			found.push(Violation {
				limit: Limit::Width,
				message: format!(
					"the width of {} px is over the {} px limit; reduce the width to {} px",
					self.width, limits.max_width, limits.max_width
				),
				suggested_duration: None,
				suggested_width: Some(limits.max_width),
			});
		}
		if let Some(max_height) = limits.max_height
			&& self.height > max_height
		{
			let width = height_at(max_height, self.height, self.width).min(self.width);
			found.push(Violation {
				limit: Limit::Height,
				message: format!(
					"the height of {} px is over the {} px limit; reduce the width to {width} px",
					self.height, max_height
				),
				suggested_duration: None,
				suggested_width: Some(width),
			});
		}
		if self.estimated_bytes > limits.max_bytes {
			found.push(self.size_violation(limits.max_bytes));
		}
		found
	}

	/// The size violation: shorten, narrow, or when neither gets under
	/// `max_bytes`, lower the frame rate or quality.
	fn size_violation(&self, max_bytes: u64) -> Violation {
		let fits = |duration: Duration, width: u32| {
			let height = height_at(width, self.width, self.height);
			estimate_bytes(frames_in(duration, self.fps), width, height, self.quality) <= max_bytes
		};
		// Whole seconds down from the current length; at least one second.
		let duration = (1..self.duration.as_secs().max(1))
			.rev()
			.map(Duration::from_secs)
			.find(|d| fits(*d, self.width));
		// Multiples of 10 px down from the current width; at least MIN_WIDTH.
		let width = (MIN_WIDTH..self.width)
			.rev()
			.filter(|w| w % 10 == 0 || *w == MIN_WIDTH)
			.find(|w| fits(self.duration, *w));
		let advice = match (duration, width) {
			(Some(d), Some(w)) => format!(
				"shorten the range to {} or reduce the width to {w} px",
				secs(d)
			),
			(Some(d), None) => format!("shorten the range to {}", secs(d)),
			(None, Some(w)) => format!("reduce the width to {w} px"),
			(None, None) => "lower the frame rate or the quality".to_owned(),
		};
		Violation {
			limit: Limit::Size,
			message: format!(
				"the estimated size of {} is over the {} limit; {advice}",
				megabytes(self.estimated_bytes),
				megabytes(max_bytes)
			),
			suggested_duration: duration,
			suggested_width: width,
		}
	}

	/// The ffmpeg command that makes the GIF; see [`command::video_to_gif`].
	#[must_use]
	pub fn command(&self, input: &Path, output: &Path) -> FfmpegCommand {
		command::video_to_gif(input, output, self)
	}
}

/// Seconds for a message: `10 s`, or `7.5 s` when not whole.
fn secs(duration: Duration) -> String {
	let ms = duration.as_millis();
	if ms.is_multiple_of(1000) {
		format!("{} s", ms / 1000)
	} else {
		format!("{:.1} s", duration.as_secs_f64())
	}
}

/// Megabytes for a message: `8 MB`, or `12.3 MB` when not whole.
fn megabytes(bytes: u64) -> String {
	if bytes.is_multiple_of(MB) {
		format!("{} MB", bytes / MB)
	} else {
		#[expect(
			clippy::cast_precision_loss,
			reason = "file sizes are far below 2^53 bytes"
		)]
		let mb = bytes as f64 / MB as f64;
		format!("{mb:.1} MB")
	}
}

/// The encoder and filters a GIF job needs that `caps` lacks. Empty when
/// the build can make GIFs.
#[must_use]
pub fn missing_for_gif(caps: &Capabilities) -> Vec<&'static str> {
	let mut missing = Vec::new();
	if !caps.has_encoder(GIF_ENCODER) {
		missing.push(GIF_ENCODER);
	}
	missing.extend(GIF_FILTERS.iter().copied().filter(|f| !caps.has_filter(f)));
	missing
}

/// The container and codec family a GIF is converted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFormat {
	/// MP4 with H.264.
	Mp4,
	/// `WebM` with VP9, or VP8 when the build lacks VP9.
	Webm,
}

impl VideoFormat {
	/// The encoders that can write this format, best first.
	#[must_use]
	pub const fn encoders(self) -> &'static [VideoEncoder] {
		match self {
			Self::Mp4 => &[VideoEncoder::X264],
			Self::Webm => &[VideoEncoder::Vp9, VideoEncoder::Vp8],
		}
	}

	/// The best encoder for this format that `caps` has, or `None` when the
	/// build cannot write it.
	#[must_use]
	pub fn encoder(self, caps: &Capabilities) -> Option<VideoEncoder> {
		self.encoders()
			.iter()
			.copied()
			.find(|e| caps.has_encoder(e.name()))
	}

	/// The `-f` muxer name.
	#[must_use]
	pub const fn muxer(self) -> &'static str {
		match self {
			Self::Mp4 => "mp4",
			Self::Webm => "webm",
		}
	}
}

impl fmt::Display for VideoFormat {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::Mp4 => "MP4",
			Self::Webm => "WebM",
		})
	}
}

/// An ffmpeg encoder [`command::gif_to_video`] knows the right options for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoEncoder {
	/// `libx264`, H.264 in MP4.
	X264,
	/// `libvpx-vp9`, VP9 in `WebM`.
	Vp9,
	/// `libvpx`, VP8 in `WebM`.
	Vp8,
}

impl VideoEncoder {
	/// The name to pass to `-c:v`, as `ffmpeg -encoders` lists it.
	#[must_use]
	pub const fn name(self) -> &'static str {
		match self {
			Self::X264 => "libx264",
			Self::Vp9 => "libvpx-vp9",
			Self::Vp8 => "libvpx",
		}
	}

	/// The format this encoder writes.
	#[must_use]
	pub const fn format(self) -> VideoFormat {
		match self {
			Self::X264 => VideoFormat::Mp4,
			Self::Vp9 | Self::Vp8 => VideoFormat::Webm,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::capabilities::{Codec, CodecKind, Filter};

	const fn source() -> Source {
		Source {
			width: 1920,
			height: 800,
			frame_rate: Some(24.0),
			duration: Some(Duration::from_secs(90)),
		}
	}

	fn request() -> Request {
		Request {
			start: Duration::from_secs(12),
			end: Duration::from_secs(20),
			fps: 0,
			width: 0,
			quality: Quality::Balanced,
			play_once: false,
		}
	}

	#[test]
	fn a_plan_scales_to_the_width_and_counts_frames() {
		let plan = plan(&request(), &source()).unwrap();
		assert_eq!((plan.fps, plan.width, plan.height), (12, 480, 200));
		assert_eq!(plan.duration, Duration::from_secs(8));
		assert_eq!(plan.frames, 96);
		assert_eq!(
			plan.estimated_bytes,
			estimate_bytes(96, 480, 200, Quality::Balanced)
		);
		// 96 frames of 480×200 at 0.45 bytes per pixel: about 4.1 MB.
		assert!((4_100_000..4_200_000).contains(&plan.estimated_bytes));
	}

	#[test]
	fn the_plan_never_exceeds_the_source() {
		let small = Source {
			width: 320,
			height: 240,
			frame_rate: Some(10.0),
			duration: Some(Duration::from_secs(5)),
		};
		let plan = plan(
			&Request {
				start: Duration::ZERO,
				end: Duration::from_secs(8),
				fps: 30,
				width: 640,
				..request()
			},
			&small,
		)
		.unwrap();
		assert_eq!((plan.fps, plan.width, plan.height), (10, 320, 240));
		assert_eq!(plan.duration, Duration::from_secs(5));
		assert_eq!(plan.frames, 50);
	}

	#[test]
	fn the_range_is_cut_at_the_end_of_the_input() {
		let short = Source {
			duration: Some(Duration::from_millis(15_500)),
			..source()
		};
		let cut = plan(&request(), &short).unwrap();
		assert_eq!(cut.duration, Duration::from_millis(3_500));
		assert_eq!(cut.frames, 42);
		// A source without a known duration is taken at its word.
		let unknown = Source {
			duration: None,
			..source()
		};
		assert_eq!(
			plan(&request(), &unknown).unwrap().duration,
			Duration::from_secs(8)
		);
	}

	#[test]
	fn fractional_frame_rates_round_up_and_odd_ranges_count_the_last_frame() {
		let ntsc = Source {
			frame_rate: Some(23.976),
			..source()
		};
		let plan = plan(
			&Request {
				fps: 24,
				..request()
			},
			&ntsc,
		)
		.unwrap();
		assert_eq!(plan.fps, 24);
		assert_eq!(frames_in(Duration::from_millis(1_001), 12), 13);
		assert_eq!(frames_in(Duration::from_millis(999), 12), 12);
	}

	#[test]
	fn impossible_requests_are_errors() {
		let bad = |r: Request| plan(&r, &source()).unwrap_err();
		assert_eq!(
			bad(Request {
				end: Duration::from_secs(12),
				..request()
			}),
			PlanError::EmptyRange
		);
		assert_eq!(
			bad(Request {
				start: Duration::from_secs(90),
				end: Duration::from_secs(91),
				..request()
			}),
			PlanError::StartPastEnd
		);
		assert_eq!(
			bad(Request {
				fps: 51,
				..request()
			}),
			PlanError::FrameRate
		);
		assert_eq!(
			bad(Request {
				width: 8,
				..request()
			}),
			PlanError::Width
		);
		assert_eq!(
			bad(Request {
				width: 5000,
				..request()
			}),
			PlanError::Width
		);
		assert_eq!(
			PlanError::Width.to_string(),
			"the width must be between 16 and 4096 px"
		);
	}

	#[test]
	fn a_plan_within_the_defaults_has_no_violations() {
		let plan = plan(&request(), &source()).unwrap();
		assert!(plan.within(&Limits::default()));
	}

	#[test]
	fn each_limit_gets_its_own_violation_and_suggestion() {
		let plan = plan(
			&Request {
				start: Duration::ZERO,
				end: Duration::from_secs(15),
				width: 960,
				..request()
			},
			&source(),
		)
		.unwrap();
		let limits = Limits {
			max_height: Some(300),
			..Limits::default()
		};
		let found = plan.violations(&limits);
		let limits_hit: Vec<Limit> = found.iter().map(|v| v.limit).collect();
		assert_eq!(
			limits_hit,
			[Limit::Duration, Limit::Width, Limit::Height, Limit::Size]
		);
		assert_eq!(
			found[0].message,
			"the range is 15 s long, over the 10 s limit; shorten it to 10 s"
		);
		assert_eq!(found[0].suggested_duration, Some(Duration::from_secs(10)));
		assert_eq!(
			found[1].message,
			"the width of 960 px is over the 480 px limit; reduce the width to 480 px"
		);
		assert_eq!(found[1].suggested_width, Some(480));
		assert_eq!(
			found[2].message,
			"the height of 400 px is over the 300 px limit; reduce the width to 720 px"
		);
		assert_eq!(found[2].suggested_width, Some(720));
		assert!(
			found[3]
				.message
				.starts_with("the estimated size of 31.1 MB is over the 8 MB limit; shorten the range to 3 s or reduce the width to 480 px"),
			"{}",
			found[3].message
		);
		assert_eq!(found[3].suggested_duration, Some(Duration::from_secs(3)));
		assert_eq!(found[3].suggested_width, Some(480));
		assert_eq!(found[3].to_string(), found[3].message);
	}

	#[test]
	fn size_suggestions_really_fit() {
		let plan = plan(
			&Request {
				start: Duration::ZERO,
				end: Duration::from_secs(10),
				..request()
			},
			&source(),
		)
		.unwrap();
		let limits = Limits {
			max_bytes: 3 * MB,
			..Limits::default()
		};
		let [violation] = &plan.violations(&limits)[..] else {
			panic!("only the size is over: {:?}", plan.violations(&limits));
		};
		let shorter = super::plan(
			&Request {
				start: Duration::ZERO,
				end: violation.suggested_duration.unwrap(),
				..request()
			},
			&source(),
		)
		.unwrap();
		assert!(shorter.estimated_bytes <= limits.max_bytes, "{shorter:?}");
		let narrower = super::plan(
			&Request {
				start: Duration::ZERO,
				end: Duration::from_secs(10),
				width: violation.suggested_width.unwrap(),
				..request()
			},
			&source(),
		)
		.unwrap();
		assert!(narrower.estimated_bytes <= limits.max_bytes, "{narrower:?}");
	}

	#[test]
	fn a_hopeless_size_limit_suggests_the_other_knobs() {
		let plan = plan(&request(), &source()).unwrap();
		let limits = Limits {
			max_bytes: 1000,
			..Limits::default()
		};
		let found = plan.violations(&limits);
		assert_eq!(found.len(), 1);
		assert!(
			found[0]
				.message
				.ends_with("lower the frame rate or the quality"),
			"{}",
			found[0].message
		);
		assert_eq!(
			(found[0].suggested_duration, found[0].suggested_width),
			(None, None)
		);
	}

	#[test]
	fn messages_format_seconds_and_megabytes_plainly() {
		assert_eq!(secs(Duration::from_secs(10)), "10 s");
		assert_eq!(secs(Duration::from_millis(7_500)), "7.5 s");
		assert_eq!(megabytes(8 * MB), "8 MB");
		assert_eq!(megabytes(12_345_678), "12.3 MB");
	}

	#[test]
	fn quality_trades_colors_for_bytes() {
		assert!(Quality::Small.max_colors() < Quality::Balanced.max_colors());
		assert!(Quality::Balanced.max_colors() < Quality::Best.max_colors());
		assert!(Quality::Small.bytes_per_pixel() < Quality::Best.bytes_per_pixel());
		assert_eq!(Quality::default(), Quality::Balanced);
		assert!(
			estimate_bytes(96, 480, 200, Quality::Small)
				< estimate_bytes(96, 480, 200, Quality::Best)
		);
	}

	fn caps(encoders: &[&str], filters: &[&str]) -> Capabilities {
		Capabilities {
			version: "test".into(),
			encoders: encoders
				.iter()
				.map(|name| Codec {
					name: (*name).to_owned(),
					kind: CodecKind::Video,
					description: String::new(),
				})
				.collect(),
			decoders: Vec::new(),
			formats: Vec::new(),
			filters: filters
				.iter()
				.map(|name| Filter {
					name: (*name).to_owned(),
					description: String::new(),
					timeline: false,
				})
				.collect(),
			configuration: Vec::new(),
			hwaccels: Vec::new(),
			bitstream_filters: Vec::new(),
		}
	}

	#[test]
	fn a_gif_needs_the_encoder_and_the_palette_filters() {
		let full = caps(&["gif"], &GIF_FILTERS);
		assert_eq!(missing_for_gif(&full), [] as [&str; 0]);
		let bare = caps(&["libx264"], &["scale", "fps"]);
		assert_eq!(
			missing_for_gif(&bare),
			["gif", "split", "palettegen", "paletteuse"]
		);
	}

	#[test]
	fn the_video_encoder_follows_the_build() {
		let both = caps(&["libx264", "libvpx", "libvpx-vp9"], &[]);
		assert_eq!(VideoFormat::Mp4.encoder(&both), Some(VideoEncoder::X264));
		assert_eq!(VideoFormat::Webm.encoder(&both), Some(VideoEncoder::Vp9));
		let vp8_only = caps(&["libvpx"], &[]);
		assert_eq!(
			VideoFormat::Webm.encoder(&vp8_only),
			Some(VideoEncoder::Vp8)
		);
		assert_eq!(VideoFormat::Mp4.encoder(&vp8_only), None);
		assert_eq!(VideoEncoder::Vp8.format(), VideoFormat::Webm);
		assert_eq!(VideoFormat::Webm.to_string(), "WebM");
		assert_eq!(VideoFormat::Mp4.muxer(), "mp4");
	}

	#[test]
	fn the_source_is_the_first_sized_video_stream() {
		let probe = crate::ffmpeg::probe::parse(
			r#"{"format":{"duration":"90.5"},"streams":[
				{"index":0,"codec_type":"audio"},
				{"index":1,"codec_type":"video","width":0,"height":0},
				{"index":2,"codec_type":"video","width":1280,"height":720,"avg_frame_rate":"30000/1001"}
			]}"#,
		)
		.unwrap();
		let source = Source::from_probe(&probe).unwrap();
		assert_eq!((source.width, source.height), (1280, 720));
		assert!((source.frame_rate.unwrap() - 29.97).abs() < 0.01);
		assert_eq!(source.duration, Some(Duration::from_millis(90_500)));

		let audio =
			crate::ffmpeg::probe::parse(r#"{"streams":[{"index":0,"codec_type":"audio"}]}"#)
				.unwrap();
		assert_eq!(Source::from_probe(&audio), None);
	}
}
