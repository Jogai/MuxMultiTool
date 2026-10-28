//! Typed view of ffprobe's JSON output.
//!
//! The runner calls `ffprobe -print_format json -show_format -show_streams
//! -show_chapters` and parses the result into a [`ProbeResult`]. ffprobe
//! prints most numbers as strings (`"duration": "12.500000"`) and uses
//! `"N/A"` for unknown values; the field deserializers here accept strings,
//! numbers and `N/A` alike, so a stream is never rejected for a field we do
//! not need. Fields ffprobe prints that are not listed here are ignored.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::de::{self, Deserializer};

/// ffprobe output that could not be parsed.
#[derive(Debug, thiserror::Error)]
#[error("ffprobe output is not the expected JSON: {0}")]
pub struct ProbeError(#[from] serde_json::Error);

/// Parses the JSON that ffprobe writes to stdout.
///
/// # Errors
/// When `json` is not valid JSON or a known field has an unexpected shape.
pub fn parse(json: &str) -> Result<ProbeResult, ProbeError> {
	Ok(serde_json::from_str(json)?)
}

/// Everything ffprobe reports about one file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ProbeResult {
	/// The container.
	#[serde(default)]
	pub format: Format,
	/// All streams in container order.
	#[serde(default)]
	pub streams: Vec<Stream>,
	/// Chapters, if the container has any.
	#[serde(default)]
	pub chapters: Vec<Chapter>,
}

impl ProbeResult {
	/// The streams of one type, in container order.
	pub fn streams_of(&self, kind: CodecType) -> impl Iterator<Item = &Stream> {
		self.streams.iter().filter(move |s| s.codec_type == kind)
	}

	/// Duration of the container, falling back to the longest stream.
	#[must_use]
	pub fn duration_secs(&self) -> Option<f64> {
		self.format.duration.or_else(|| {
			self.streams
				.iter()
				.filter_map(|s| s.duration)
				.reduce(f64::max)
		})
	}
}

/// The container (`-show_format`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Format {
	/// Path as given to ffprobe.
	#[serde(default)]
	pub filename: String,
	/// Number of streams.
	#[serde(default)]
	pub nb_streams: u32,
	/// Short demuxer name, possibly a comma-separated list (`mov,mp4,m4a,...`).
	#[serde(default)]
	pub format_name: String,
	/// Human-readable demuxer name.
	#[serde(default)]
	pub format_long_name: String,
	/// Start time in seconds.
	#[serde(default, deserialize_with = "lenient")]
	pub start_time: Option<f64>,
	/// Duration in seconds.
	#[serde(default, deserialize_with = "lenient")]
	pub duration: Option<f64>,
	/// File size in bytes.
	#[serde(default, deserialize_with = "lenient")]
	pub size: Option<u64>,
	/// Overall bitrate in bit/s.
	#[serde(default, deserialize_with = "lenient")]
	pub bit_rate: Option<u64>,
	/// How sure the demuxer is about the format, 0–100.
	#[serde(default, deserialize_with = "lenient")]
	pub probe_score: Option<u32>,
	/// Container metadata such as `title` or `encoder`.
	#[serde(default)]
	pub tags: BTreeMap<String, String>,
}

/// The kind of data a stream carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodecType {
	/// Pictures.
	Video,
	/// Sound.
	Audio,
	/// Text or bitmap subtitles.
	Subtitle,
	/// Timed data such as timecodes or MPEG-TS tables.
	Data,
	/// Embedded files, usually fonts for ASS subtitles.
	Attachment,
	/// A type this version of the parser does not know.
	#[default]
	#[serde(other)]
	Unknown,
}

impl fmt::Display for CodecType {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::Video => "video",
			Self::Audio => "audio",
			Self::Subtitle => "subtitle",
			Self::Data => "data",
			Self::Attachment => "attachment",
			Self::Unknown => "unknown",
		})
	}
}

/// One stream (`-show_streams`). Fields that only apply to one codec type
/// are `None` for the others.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Stream {
	/// Position in the container, what `-map 0:<index>` refers to.
	#[serde(default)]
	pub index: u32,
	/// What the stream carries.
	#[serde(default)]
	pub codec_type: CodecType,
	/// Short codec name (`h264`, `aac`), or `None` for unknown codecs.
	#[serde(default)]
	pub codec_name: Option<String>,
	/// Human-readable codec name.
	#[serde(default)]
	pub codec_long_name: Option<String>,
	/// Codec profile (`High`, `LC`), when the codec has profiles.
	#[serde(default, deserialize_with = "lenient")]
	pub profile: Option<String>,
	/// Codec level as the codec defines it (`31` for H.264 level 3.1).
	#[serde(default, deserialize_with = "lenient")]
	pub level: Option<i64>,
	/// Four-character code as text (`avc1`).
	#[serde(default)]
	pub codec_tag_string: Option<String>,
	/// Picture width in pixels.
	#[serde(default, deserialize_with = "lenient")]
	pub width: Option<u32>,
	/// Picture height in pixels.
	#[serde(default, deserialize_with = "lenient")]
	pub height: Option<u32>,
	/// Pixel format (`yuv420p`).
	#[serde(default)]
	pub pix_fmt: Option<String>,
	/// Sample aspect ratio as `w:h`.
	#[serde(default)]
	pub sample_aspect_ratio: Option<String>,
	/// Display aspect ratio as `w:h`.
	#[serde(default)]
	pub display_aspect_ratio: Option<String>,
	/// Interlacing (`progressive`, `tt`, `bb`, ...).
	#[serde(default)]
	pub field_order: Option<String>,
	/// Real base frame rate, which can be higher than the average for
	/// variable frame rate material.
	#[serde(default, deserialize_with = "lenient")]
	pub r_frame_rate: Option<Rational>,
	/// Average frame rate.
	#[serde(default, deserialize_with = "lenient")]
	pub avg_frame_rate: Option<Rational>,
	/// Unit of `start_pts` and `duration_ts`.
	#[serde(default, deserialize_with = "lenient")]
	pub time_base: Option<Rational>,
	/// First timestamp in `time_base` units.
	#[serde(default, deserialize_with = "lenient")]
	pub start_pts: Option<i64>,
	/// First timestamp in seconds.
	#[serde(default, deserialize_with = "lenient")]
	pub start_time: Option<f64>,
	/// Duration in `time_base` units.
	#[serde(default, deserialize_with = "lenient")]
	pub duration_ts: Option<i64>,
	/// Duration in seconds.
	#[serde(default, deserialize_with = "lenient")]
	pub duration: Option<f64>,
	/// Bitrate in bit/s.
	#[serde(default, deserialize_with = "lenient")]
	pub bit_rate: Option<u64>,
	/// Bits per sample before any packing (`8`, `10`).
	#[serde(default, deserialize_with = "lenient")]
	pub bits_per_raw_sample: Option<u32>,
	/// Number of frames, when the container knows it.
	#[serde(default, deserialize_with = "lenient")]
	pub nb_frames: Option<u64>,
	/// Audio sample format (`fltp`).
	#[serde(default)]
	pub sample_fmt: Option<String>,
	/// Audio sample rate in Hz.
	#[serde(default, deserialize_with = "lenient")]
	pub sample_rate: Option<u32>,
	/// Number of audio channels.
	#[serde(default, deserialize_with = "lenient")]
	pub channels: Option<u32>,
	/// Audio channel layout (`stereo`, `5.1`).
	#[serde(default)]
	pub channel_layout: Option<String>,
	/// Default/forced/... flags.
	#[serde(default)]
	pub disposition: Disposition,
	/// Stream metadata such as `language` or `title`.
	#[serde(default)]
	pub tags: BTreeMap<String, String>,
}

impl Stream {
	/// Frame rate in frames per second, preferring the average rate.
	#[must_use]
	pub fn frame_rate(&self) -> Option<f64> {
		self.avg_frame_rate
			.filter(|r| r.num != 0)
			.or(self.r_frame_rate)
			.and_then(Rational::as_f64)
	}

	/// The `language` tag, when present.
	#[must_use]
	pub fn language(&self) -> Option<&str> {
		self.tags.get("language").map(String::as_str)
	}
}

/// Stream flags (`-show_streams` disposition block).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[expect(clippy::struct_excessive_bools, reason = "mirrors ffprobe's flag set")]
pub struct Disposition {
	/// Selected by default for its type.
	#[serde(default, deserialize_with = "int_bool")]
	pub default: bool,
	/// Dubbed audio.
	#[serde(default, deserialize_with = "int_bool")]
	pub dub: bool,
	/// Original-language audio.
	#[serde(default, deserialize_with = "int_bool")]
	pub original: bool,
	/// Commentary track.
	#[serde(default, deserialize_with = "int_bool")]
	pub comment: bool,
	/// Subtitles that must always be shown.
	#[serde(default, deserialize_with = "int_bool")]
	pub forced: bool,
	/// For hearing-impaired audiences.
	#[serde(default, deserialize_with = "int_bool")]
	pub hearing_impaired: bool,
	/// For visually-impaired audiences.
	#[serde(default, deserialize_with = "int_bool")]
	pub visual_impaired: bool,
	/// A single picture, e.g. cover art, not a video track.
	#[serde(default, deserialize_with = "int_bool")]
	pub attached_pic: bool,
	/// Closed captions.
	#[serde(default, deserialize_with = "int_bool")]
	pub captions: bool,
	/// Audio descriptions.
	#[serde(default, deserialize_with = "int_bool")]
	pub descriptions: bool,
}

/// A chapter (`-show_chapters`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Chapter {
	/// Chapter id as stored in the container.
	#[serde(default)]
	pub id: i64,
	/// Unit of `start` and `end`.
	#[serde(default, deserialize_with = "lenient")]
	pub time_base: Option<Rational>,
	/// Start in `time_base` units.
	#[serde(default)]
	pub start: i64,
	/// Start in seconds.
	#[serde(default, deserialize_with = "lenient")]
	pub start_time: Option<f64>,
	/// End in `time_base` units.
	#[serde(default)]
	pub end: i64,
	/// End in seconds.
	#[serde(default, deserialize_with = "lenient")]
	pub end_time: Option<f64>,
	/// Chapter metadata, usually a `title`.
	#[serde(default)]
	pub tags: BTreeMap<String, String>,
}

/// A fraction as ffprobe prints it: `30000/1001`, `1/90000` or `16:9`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational {
	/// Numerator.
	pub num: i64,
	/// Denominator; `0` means undefined (`0/0`).
	pub den: i64,
}

impl Rational {
	/// The value as a float, or `None` when the denominator is zero.
	#[must_use]
	pub fn as_f64(self) -> Option<f64> {
		#[expect(clippy::cast_precision_loss, reason = "ffprobe fractions are small")]
		(self.den != 0).then(|| self.num as f64 / self.den as f64)
	}
}

impl FromStr for Rational {
	type Err = &'static str;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let (num, den) = s.split_once(['/', ':']).ok_or("expected num/den")?;
		let num = num.trim().parse().map_err(|_| "invalid numerator")?;
		let den = den.trim().parse().map_err(|_| "invalid denominator")?;
		Ok(Self { num, den })
	}
}

impl fmt::Display for Rational {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}/{}", self.num, self.den)
	}
}

/// A JSON scalar in the shapes ffprobe uses for one value.
#[derive(Deserialize)]
#[serde(untagged)]
enum Scalar {
	Number(serde_json::Number),
	Text(String),
	Bool(bool),
}

/// Deserializes a value ffprobe may print as a string, a number, `N/A` or
/// `null` into `Option<T>`.
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
	D: Deserializer<'de>,
	T: FromStr,
{
	let Some(scalar) = Option::<Scalar>::deserialize(deserializer)? else {
		return Ok(None);
	};
	let text = match scalar {
		Scalar::Number(n) => n.to_string(),
		Scalar::Text(s) => s,
		Scalar::Bool(b) => b.to_string(),
	};
	if text.is_empty() || text == "N/A" {
		return Ok(None);
	}
	text.parse().map(Some).map_err(|_| {
		de::Error::custom(format!(
			"cannot read {text:?} as {}",
			std::any::type_name::<T>()
		))
	})
}

/// Deserializes ffprobe's `0`/`1` flags into a bool.
fn int_bool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
	Ok(match Scalar::deserialize(deserializer)? {
		Scalar::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
		Scalar::Text(s) => s != "0" && !s.is_empty() && s != "false",
		Scalar::Bool(b) => b,
	})
}

#[cfg(test)]
mod tests {
	use proptest::prelude::*;

	use super::*;

	/// Trimmed real output of `ffprobe -show_format -show_streams -show_chapters`.
	const SAMPLE: &str = r#"{
		"streams": [
			{
				"index": 0, "codec_name": "h264", "codec_long_name": "H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10",
				"profile": "High", "codec_type": "video", "codec_tag_string": "avc1", "codec_tag": "0x31637661",
				"width": 1920, "height": 1080, "coded_width": 1920, "coded_height": 1088, "has_b_frames": 2,
				"sample_aspect_ratio": "1:1", "display_aspect_ratio": "16:9", "pix_fmt": "yuv420p", "level": 40,
				"field_order": "progressive", "refs": 1, "is_avc": "true", "nal_length_size": "4",
				"r_frame_rate": "24000/1001", "avg_frame_rate": "24000/1001", "time_base": "1/24000",
				"start_pts": 0, "start_time": "0.000000", "duration_ts": 2882880, "duration": "120.120000",
				"bit_rate": "4997521", "bits_per_raw_sample": "8", "nb_frames": "2880",
				"disposition": { "default": 1, "dub": 0, "original": 0, "comment": 0, "lyrics": 0, "karaoke": 0,
					"forced": 0, "hearing_impaired": 0, "visual_impaired": 0, "clean_effects": 0, "attached_pic": 0,
					"timed_thumbnails": 0, "captions": 0, "descriptions": 0, "metadata": 0, "dependent": 0, "still_image": 0 },
				"tags": { "language": "und", "handler_name": "VideoHandler", "vendor_id": "[0][0][0][0]" }
			},
			{
				"index": 1, "codec_name": "aac", "profile": "LC", "codec_type": "audio", "sample_fmt": "fltp",
				"sample_rate": "48000", "channels": 2, "channel_layout": "stereo", "bits_per_sample": 0,
				"r_frame_rate": "0/0", "avg_frame_rate": "0/0", "time_base": "1/48000", "start_pts": 0,
				"start_time": "0.000000", "duration_ts": 5765760, "duration": "120.120000", "bit_rate": "128000",
				"nb_frames": "5631", "disposition": { "default": 1, "forced": 0 }, "tags": { "language": "eng" }
			},
			{
				"index": 2, "codec_name": "subrip", "codec_type": "subtitle", "r_frame_rate": "0/0",
				"time_base": "1/1000", "duration_ts": 120120, "duration": "120.120000", "bit_rate": "N/A",
				"disposition": { "default": 0, "forced": 1 }, "tags": { "language": "nld", "title": "Dutch" }
			}
		],
		"chapters": [
			{ "id": 0, "time_base": "1/1000000000", "start": 0, "start_time": "0.000000", "end": 60000000000,
				"end_time": "60.000000", "tags": { "title": "Intro" } }
		],
		"format": {
			"filename": "/media/in/movie.mkv", "nb_streams": 3, "nb_programs": 0, "format_name": "matroska,webm",
			"format_long_name": "Matroska / WebM", "start_time": "0.000000", "duration": "120.120000",
			"size": "76979200", "bit_rate": "5127000", "probe_score": 100,
			"tags": { "title": "Movie", "ENCODER": "Lavf61.7.100" }
		}
	}"#;

	#[test]
	fn parses_real_output() {
		let probe = parse(SAMPLE).unwrap();
		assert_eq!(probe.format.format_name, "matroska,webm");
		assert_eq!(probe.format.duration, Some(120.12));
		assert_eq!(probe.format.size, Some(76_979_200));
		assert_eq!(probe.format.tags["title"], "Movie");
		assert_eq!(probe.duration_secs(), Some(120.12));
		assert_eq!(probe.streams.len(), 3);

		let video = &probe.streams[0];
		assert_eq!(video.codec_type, CodecType::Video);
		assert_eq!(video.codec_name.as_deref(), Some("h264"));
		assert_eq!(video.profile.as_deref(), Some("High"));
		assert_eq!(video.level, Some(40));
		assert_eq!((video.width, video.height), (Some(1920), Some(1080)));
		assert_eq!(
			video.r_frame_rate,
			Some(Rational {
				num: 24000,
				den: 1001
			})
		);
		assert!((video.frame_rate().unwrap() - 23.976).abs() < 0.001);
		assert_eq!(video.nb_frames, Some(2880));
		assert_eq!(video.bits_per_raw_sample, Some(8));
		assert!(video.disposition.default);
		assert!(!video.disposition.forced);

		let audio = probe.streams_of(CodecType::Audio).next().unwrap();
		assert_eq!(audio.sample_rate, Some(48_000));
		assert_eq!(audio.channels, Some(2));
		assert_eq!(audio.language(), Some("eng"));
		assert_eq!(audio.frame_rate(), None);

		let subs = &probe.streams[2];
		assert_eq!(subs.codec_type, CodecType::Subtitle);
		assert_eq!(subs.bit_rate, None);
		assert!(subs.disposition.forced);

		let chapter = &probe.chapters[0];
		assert_eq!(chapter.end, 60_000_000_000);
		assert_eq!(chapter.end_time, Some(60.0));
		assert_eq!(chapter.tags["title"], "Intro");
	}

	#[test]
	fn unknown_codec_type_and_profile_number() {
		let probe =
			parse(r#"{"streams":[{"index":0,"codec_type":"whatever","profile":100}]}"#).unwrap();
		assert_eq!(probe.streams[0].codec_type, CodecType::Unknown);
		assert_eq!(probe.streams[0].profile.as_deref(), Some("100"));
	}

	#[test]
	fn empty_object_is_a_valid_result() {
		let probe = parse("{}").unwrap();
		assert_eq!(probe, ProbeResult::default());
		assert_eq!(probe.duration_secs(), None);
	}

	#[test]
	fn duration_falls_back_to_streams() {
		let probe =
			parse(r#"{"streams":[{"duration":"3.5"},{"duration":"N/A"},{"duration":"7.25"}]}"#)
				.unwrap();
		assert_eq!(probe.duration_secs(), Some(7.25));
	}

	#[test]
	fn bad_number_is_an_error() {
		let err = parse(r#"{"format":{"size":"lots"}}"#).unwrap_err();
		assert!(err.to_string().contains("lots"), "{err}");
		assert!(parse("not json").is_err());
	}

	#[test]
	fn rational_from_str() {
		assert_eq!(
			"30000/1001".parse(),
			Ok(Rational {
				num: 30000,
				den: 1001
			})
		);
		assert_eq!("16:9".parse(), Ok(Rational { num: 16, den: 9 }));
		assert_eq!(Rational { num: 0, den: 0 }.as_f64(), None);
		assert!("25".parse::<Rational>().is_err());
		assert!("a/b".parse::<Rational>().is_err());
	}

	/// Seconds with six decimals, the way ffprobe prints them, and the exact
	/// float the parser must produce.
	fn seconds() -> impl Strategy<Value = (String, f64)> {
		(0u64..=100_000_000_000).prop_map(|micros| {
			#[expect(clippy::cast_precision_loss, reason = "below 2^53")]
			let value = micros as f64 / 1e6;
			(
				format!("{}.{:06}", micros / 1_000_000, micros % 1_000_000),
				value,
			)
		})
	}

	fn rational() -> impl Strategy<Value = Rational> {
		(any::<i64>(), any::<i64>()).prop_map(|(num, den)| Rational { num, den })
	}

	fn tag() -> impl Strategy<Value = (String, String)> {
		("[a-zA-Z_]{1,12}", "[a-zA-Z0-9 _.,/()éü-]{0,24}")
	}

	fn json_tags(tags: &[(String, String)]) -> String {
		tags.iter()
			.map(|(k, v)| format!("{k:?}:{v:?}"))
			.collect::<Vec<_>>()
			.join(",")
	}

	#[derive(Debug, Clone, Copy)]
	enum Shape {
		Str,
		Num,
		NotAvailable,
	}

	fn shape() -> impl Strategy<Value = Shape> {
		prop_oneof![
			Just(Shape::Str),
			Just(Shape::Num),
			Just(Shape::NotAvailable)
		]
	}

	/// Formats a number the way `shape` says ffprobe would print it.
	fn shaped<T: fmt::Display>(value: T, shape: Shape) -> (String, Option<T>) {
		match shape {
			Shape::Str => (format!("\"{value}\""), Some(value)),
			Shape::Num => (value.to_string(), Some(value)),
			Shape::NotAvailable => (String::from("\"N/A\""), None),
		}
	}

	/// A stream object as JSON text and the value it must parse to.
	fn stream() -> impl Strategy<Value = (String, Stream)> {
		(
			any::<u32>(),
			prop_oneof![
				Just(("video", CodecType::Video)),
				Just(("audio", CodecType::Audio)),
				Just(("subtitle", CodecType::Subtitle)),
				Just(("attachment", CodecType::Attachment)),
				Just(("data", CodecType::Data)),
				Just(("hologram", CodecType::Unknown)),
			],
			"[a-z0-9]{1,8}",
			(any::<u32>(), shape()),
			(any::<u32>(), shape()),
			(seconds(), shape()),
			(any::<u64>(), shape()),
			(any::<u32>(), shape()),
			rational(),
			prop::collection::vec(tag(), 0..4).prop_map(|mut t| {
				t.sort();
				t.dedup_by(|a, b| a.0 == b.0);
				t
			}),
			any::<bool>(),
		)
			.prop_map(
				|(
					index,
					(type_text, codec_type),
					codec_name,
					(width, width_shape),
					(sample_rate, rate_shape),
					((duration_text, duration), duration_shape),
					(bit_rate, bit_rate_shape),
					(level, level_shape),
					time_base,
					tags,
					forced,
				)| {
					let (width_json, width) = shaped(width, width_shape);
					let (rate_json, sample_rate) = shaped(sample_rate, rate_shape);
					let (duration_json, duration) = match duration_shape {
						Shape::Str => (format!("\"{duration_text}\""), Some(duration)),
						Shape::Num => (duration_text, Some(duration)),
						Shape::NotAvailable => (String::from("\"N/A\""), None),
					};
					let (bit_rate_json, bit_rate) = shaped(bit_rate, bit_rate_shape);
					let (level_json, level) = shaped(level, level_shape);
					let json = format!(
						"{{\"index\":{index},\"codec_type\":\"{type_text}\",\"codec_name\":\"{codec_name}\",\
						 \"width\":{width_json},\"sample_rate\":{rate_json},\"duration\":{duration_json},\
						 \"bit_rate\":{bit_rate_json},\"level\":{level_json},\"time_base\":\"{time_base}\",\
						 \"unknown_field\":[1,2],\"disposition\":{{\"forced\":{},\"default\":0}},\"tags\":{{{}}}}}",
						u8::from(forced),
						json_tags(&tags),
					);
					let expected = Stream {
						index,
						codec_type,
						codec_name: Some(codec_name),
						width,
						sample_rate,
						duration,
						bit_rate,
						level: level.map(i64::from),
						time_base: Some(time_base),
						disposition: Disposition {
							forced,
							..Disposition::default()
						},
						tags: tags.into_iter().collect(),
						..Stream::default()
					};
					(json, expected)
				},
			)
	}

	fn probe_json() -> impl Strategy<Value = (String, ProbeResult)> {
		(
			prop::collection::vec(stream(), 0..5),
			(seconds(), shape()),
			prop::option::of(any::<u64>()),
		)
			.prop_map(
				|(streams, ((duration_text, duration), duration_shape), size)| {
					let (duration_json, duration) = match duration_shape {
						Shape::Str => (format!("\"{duration_text}\""), Some(duration)),
						Shape::Num => (duration_text, Some(duration)),
						Shape::NotAvailable => (String::from("\"N/A\""), None),
					};
					let size_json =
						size.map_or_else(|| String::from("null"), |s| format!("\"{s}\""));
					let json = format!(
						"{{\"streams\":[{}],\"format\":{{\"format_name\":\"mkv\",\"duration\":{duration_json},\
					 \"size\":{size_json},\"tags\":{{}}}}}}",
						streams
							.iter()
							.map(|(json, _)| json.as_str())
							.collect::<Vec<_>>()
							.join(","),
					);
					let expected = ProbeResult {
						format: Format {
							format_name: String::from("mkv"),
							duration,
							size,
							..Format::default()
						},
						streams: streams.into_iter().map(|(_, stream)| stream).collect(),
						chapters: Vec::new(),
					};
					(json, expected)
				},
			)
	}

	proptest! {
		#[test]
		fn ffprobe_json_round_trips((json, expected) in probe_json()) {
			let parsed = parse(&json).map_err(|e| TestCaseError::fail(format!("{e}\n{json}")))?;
			prop_assert_eq!(parsed, expected);
		}

		#[test]
		fn any_json_value_is_harmless(json in prop::string::string_regex("[{}\\[\\]\":,0-9a-zN/A. ]{0,64}").unwrap()) {
			let _ = parse(&json);
		}

		#[test]
		fn any_scalar_in_a_numeric_field_is_harmless(value in "[0-9a-zA-Z/:.\\-]{0,12}", quoted in any::<bool>()) {
			let json = if quoted {
				format!("{{\"format\":{{\"duration\":\"{value}\"}}}}")
			} else {
				format!("{{\"format\":{{\"duration\":{value}}}}}")
			};
			if let Ok(parsed) = parse(&json) {
				let expected = match value.as_str() {
					"" | "N/A" => None,
					v => v.parse::<f64>().ok(),
				};
				prop_assert_eq!(parsed.format.duration, expected);
			}
		}
	}
}
