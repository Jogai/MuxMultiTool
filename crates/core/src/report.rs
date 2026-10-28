//! The per-stream report: for each file, every stream's id, type, codec,
//! language and size, and on request the full ffprobe details of one
//! stream. This is what the Files screen shows for a selection of files,
//! and what the stream rules, the combine screen and the crop feature start
//! from.
//!
//! A report never fails as a whole: [`report`] answers for one file and
//! returns a [`ReportError`] for a file ffprobe cannot read, so the caller
//! can list the good files next to the bad one.
//!
//! # Stream sizes
//!
//! ffprobe does not say how many bytes a stream takes, so the report works
//! it out and says how in [`SizeMethod`], in this order of preference:
//!
//! 1. **Packets** (exact): the sizes of all packets of every stream, summed
//!    in one demux pass over the file (`ffprobe -show_entries
//!    packet=stream_index,size`). This costs one read of the file, so it
//!    runs only when a stream has no statistics tag, and never for a
//!    [quick](ReportOptions::quick) report. When it runs, every stream
//!    takes its size from it, tags included.
//! 2. **Tags** (exact when written): the Matroska statistics tag
//!    `NUMBER_OF_BYTES`, which mkvmerge writes and ffmpeg keeps when it
//!    copies a stream. A file that was cut without rewriting the tags keeps
//!    the old numbers; the packet pass is the remedy.
//! 3. **Extradata** (exact): an attachment (a font, a cover stored as an
//!    attachment) has no packets; the attached file is the stream's
//!    extradata, whose size ffprobe reports.
//! 4. **Estimated**: bit rate × duration ÷ 8 from ffprobe's per-stream
//!    values, when the packet pass did not run or failed. MP4 derives the
//!    bit rate from its sample table, so the estimate is close there;
//!    Matroska and MPEG-TS often have no per-stream bit rate at all.
//! 5. **Unknown** otherwise.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::time::Duration;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::ffmpeg::probe::{CodecType, ProbeError, ProbeResult, Stream};
use crate::ffmpeg::{Limits, RunError, Runner};
use crate::jobs::explain;

/// The Matroska statistics tag that holds a stream's size in bytes.
/// mkvmerge suffixes it with the tag language (`NUMBER_OF_BYTES-eng`).
const BYTES_TAG: &str = "NUMBER_OF_BYTES";

/// How a report is made.
#[derive(Debug, Clone)]
pub struct ReportOptions {
	/// Skip the packet pass: sizes come from tags or estimates only, so the
	/// report is as fast as ffprobe reads the headers.
	pub quick: bool,
	/// How long one ffprobe metadata run may take.
	pub probe_timeout: Duration,
	/// How long the packet pass over one file may take. When it runs out,
	/// the sizes fall back to estimates.
	pub measure_timeout: Duration,
	/// Stops every ffprobe run.
	pub cancel: CancellationToken,
}

impl Default for ReportOptions {
	fn default() -> Self {
		Self {
			quick: false,
			probe_timeout: Duration::from_secs(60),
			measure_timeout: Duration::from_secs(15 * 60),
			cancel: CancellationToken::new(),
		}
	}
}

/// Where a stream's size came from; see the module docs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SizeMethod {
	/// No source for the size; it is `None`.
	#[default]
	Unknown,
	/// Summed from the packets of the stream.
	Packets,
	/// The `NUMBER_OF_BYTES` statistics tag.
	Tags,
	/// The size of the attachment's data.
	Extradata,
	/// Bit rate × duration.
	Estimated,
}

impl fmt::Display for SizeMethod {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::Unknown => "unknown",
			Self::Packets => "packets",
			Self::Tags => "tags",
			Self::Extradata => "extradata",
			Self::Estimated => "estimated",
		})
	}
}

/// The report for one file.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
	/// The container.
	pub container: Container,
	/// Every stream, in container order.
	pub streams: Vec<StreamReport>,
	/// How many chapters the container has.
	pub chapters: usize,
}

/// The container, as far as the report shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Container {
	/// Short demuxer name (`matroska,webm`).
	pub format: String,
	/// Human-readable demuxer name.
	pub format_long_name: String,
	/// Duration in seconds, falling back to the longest stream.
	pub duration: Option<f64>,
	/// File size in bytes.
	pub size: Option<u64>,
	/// Overall bitrate in bit/s.
	pub bit_rate: Option<u64>,
	/// The `title` tag.
	pub title: Option<String>,
}

/// One row of the report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamReport {
	/// Position in the container, what `-map 0:<index>` refers to.
	pub index: u32,
	/// What the stream carries.
	pub kind: CodecType,
	/// Short codec name (`h264`), or `unknown`.
	pub codec: String,
	/// Human-readable codec name.
	pub codec_long_name: Option<String>,
	/// Codec profile, when the codec has profiles.
	pub profile: Option<String>,
	/// The `language` tag as stored, including `und`.
	pub language: Option<String>,
	/// The track title: the `title` tag (Matroska) or `name` (MP4).
	pub title: Option<String>,
	/// Size in bytes, by `size_method`.
	pub size: Option<u64>,
	/// Where `size` came from.
	pub size_method: SizeMethod,
	/// Duration in seconds, falling back to the container's.
	pub duration: Option<f64>,
	/// Bitrate in bit/s.
	pub bit_rate: Option<u64>,
	/// Picture width in pixels.
	pub width: Option<u32>,
	/// Picture height in pixels.
	pub height: Option<u32>,
	/// Frame rate in frames per second.
	pub frame_rate: Option<f64>,
	/// Pixel format (`yuv420p`).
	pub pix_fmt: Option<String>,
	/// Number of audio channels.
	pub channels: Option<u32>,
	/// Audio channel layout (`stereo`, `5.1`).
	pub channel_layout: Option<String>,
	/// Audio sample rate in Hz.
	pub sample_rate: Option<u32>,
	/// The disposition flags that are set (`default`, `forced`, ...).
	pub flags: Vec<String>,
}

/// Why a file has no report. The texts never contain a server path: the
/// file name stands in for it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ReportError {
	/// Plain language, e.g. "the input is not a media file ffmpeg can read".
	pub message: String,
	/// The tail of what ffprobe wrote to stderr, empty when it did not run.
	pub ffprobe_output: String,
}

impl ReportError {
	/// Explains a runner error for `path`.
	fn from_run(error: RunError, path: &Path) -> Self {
		match error {
			RunError::Failed {
				status, log_tail, ..
			} => {
				let log_tail = hide_path(&log_tail, path);
				Self {
					message: explain(status, &log_tail),
					ffprobe_output: log_tail,
				}
			}
			other => Self {
				message: hide_path(&other.to_string(), path),
				ffprobe_output: String::new(),
			},
		}
	}
}

/// Replaces `path` in `text` with its file name.
fn hide_path(text: &str, path: &Path) -> String {
	let full = path.to_string_lossy();
	let name = path
		.file_name()
		.map_or_else(|| full.clone(), |n| n.to_string_lossy());
	text.replace(full.as_ref(), name.as_ref())
}

/// Reports on `path`: ffprobe, then the packet pass when the sizes need it
/// (see the module docs).
///
/// # Errors
/// [`ReportError`] when ffprobe cannot read the file or was stopped. A
/// failed packet pass is not an error: the sizes are estimated instead and
/// the failure is logged.
pub async fn report(
	runner: &Runner,
	path: &Path,
	options: &ReportOptions,
) -> Result<Report, ReportError> {
	let limits = Limits {
		timeout: Some(options.probe_timeout),
		cancel: options.cancel.clone(),
	};
	let probe = runner
		.probe(path, &limits)
		.await
		.map_err(|error| ReportError::from_run(error, path))?;
	let packets = if !options.quick && needs_measuring(&probe) {
		let limits = Limits {
			timeout: Some(options.measure_timeout),
			cancel: options.cancel.clone(),
		};
		match runner.packet_sizes(path, &limits).await {
			Ok(sizes) => Some(sizes),
			Err(error @ RunError::Cancelled { .. }) => {
				return Err(ReportError::from_run(error, path));
			}
			Err(error) => {
				tracing::warn!(
					target: "ffprobe",
					path = %path.display(),
					%error,
					"packet pass failed; stream sizes are estimated"
				);
				None
			}
		}
	} else {
		None
	};
	Ok(build(&probe, packets.as_ref()))
}

/// Whether some stream of `probe` would get an estimated or unknown size
/// without the packet pass.
#[must_use]
pub fn needs_measuring(probe: &ProbeResult) -> bool {
	probe
		.streams
		.iter()
		.any(|s| s.codec_type != CodecType::Attachment && bytes_tag(s).is_none())
}

/// Builds the report from a parsed probe and, when the packet pass ran,
/// the bytes per stream index it measured.
#[must_use]
pub fn build(probe: &ProbeResult, packets: Option<&BTreeMap<u32, u64>>) -> Report {
	let container_duration = probe.duration_secs();
	let streams = probe
		.streams
		.iter()
		.map(|stream| {
			let (size, size_method) = size_of(stream, packets, container_duration);
			StreamReport {
				index: stream.index,
				kind: stream.codec_type,
				codec: stream
					.codec_name
					.clone()
					.unwrap_or_else(|| String::from("unknown")),
				codec_long_name: stream.codec_long_name.clone(),
				profile: stream.profile.clone(),
				language: stream.language().map(str::to_owned),
				// Matroska stores the track title as `title`, MP4 as `name`
				title: stream
					.tags
					.get("title")
					.or_else(|| stream.tags.get("name"))
					.cloned(),
				size,
				size_method,
				duration: stream.duration.or(container_duration),
				bit_rate: stream.bit_rate,
				width: stream.width,
				height: stream.height,
				frame_rate: (stream.codec_type == CodecType::Video)
					.then(|| stream.frame_rate())
					.flatten(),
				pix_fmt: stream.pix_fmt.clone(),
				channels: stream.channels,
				channel_layout: stream.channel_layout.clone(),
				sample_rate: stream.sample_rate,
				flags: flags(stream),
			}
		})
		.collect();
	Report {
		container: Container {
			format: probe.format.format_name.clone(),
			format_long_name: probe.format.format_long_name.clone(),
			duration: container_duration,
			size: probe.format.size,
			bit_rate: probe.format.bit_rate,
			title: probe.format.tags.get("title").cloned(),
		},
		streams,
		chapters: probe.chapters.len(),
	}
}

/// The size policy for one stream; see the module docs.
fn size_of(
	stream: &Stream,
	packets: Option<&BTreeMap<u32, u64>>,
	container_duration: Option<f64>,
) -> (Option<u64>, SizeMethod) {
	if stream.codec_type == CodecType::Attachment {
		return match stream.extradata_size {
			Some(size) => (Some(size), SizeMethod::Extradata),
			None => (None, SizeMethod::Unknown),
		};
	}
	if let Some(packets) = packets {
		// The pass saw the whole file: a stream it did not see has no packets.
		return (
			Some(packets.get(&stream.index).copied().unwrap_or(0)),
			SizeMethod::Packets,
		);
	}
	if let Some(size) = bytes_tag(stream) {
		return (Some(size), SizeMethod::Tags);
	}
	if let (Some(bit_rate), Some(duration)) =
		(stream.bit_rate, stream.duration.or(container_duration))
	{
		#[expect(
			clippy::cast_precision_loss,
			clippy::cast_possible_truncation,
			clippy::cast_sign_loss,
			reason = "an estimate of a byte count"
		)]
		let size = (bit_rate as f64 * duration / 8.0).round().max(0.0) as u64;
		return (Some(size), SizeMethod::Estimated);
	}
	(None, SizeMethod::Unknown)
}

/// The `NUMBER_OF_BYTES` tag, with or without a language suffix.
fn bytes_tag(stream: &Stream) -> Option<u64> {
	stream
		.tags
		.iter()
		.find(|(key, _)| {
			key.as_str() == BYTES_TAG
				|| key
					.strip_prefix(BYTES_TAG)
					.is_some_and(|rest| rest.starts_with('-'))
		})
		.and_then(|(_, value)| value.trim().parse().ok())
}

/// The names of the disposition flags that are set.
fn flags(stream: &Stream) -> Vec<String> {
	let d = stream.disposition;
	[
		("default", d.default),
		("dub", d.dub),
		("original", d.original),
		("comment", d.comment),
		("forced", d.forced),
		("hearing_impaired", d.hearing_impaired),
		("visual_impaired", d.visual_impaired),
		("attached_pic", d.attached_pic),
		("captions", d.captions),
		("descriptions", d.descriptions),
	]
	.into_iter()
	.filter(|&(_, set)| set)
	.map(|(name, _)| name.to_owned())
	.collect()
}

/// Everything ffprobe reports about one stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamDetails {
	/// Every field as `(key, value)` in the order ffprobe printed them.
	/// Nested objects are flattened with dots (`disposition.default`,
	/// `tags.language`), arrays with indices (`side_data_list[0].side_data_type`).
	/// Values are the text ffprobe printed; `null` is an empty string.
	pub fields: Vec<(String, String)>,
	/// The stream's object from ffprobe's JSON output, pretty-printed.
	pub json: String,
}

/// The details of stream `index` of `path`.
///
/// # Errors
/// [`ReportError`] when ffprobe cannot read the file or was stopped.
/// `Ok(None)` when the file has no stream with that index.
pub async fn details(
	runner: &Runner,
	path: &Path,
	index: u32,
	options: &ReportOptions,
) -> Result<Option<StreamDetails>, ReportError> {
	let limits = Limits {
		timeout: Some(options.probe_timeout),
		cancel: options.cancel.clone(),
	};
	let json = runner
		.probe_json(path, &limits)
		.await
		.map_err(|error| ReportError::from_run(error, path))?;
	stream_details(&json, index).map_err(|error| ReportError {
		message: error.to_string(),
		ffprobe_output: String::new(),
	})
}

/// Picks stream `index` out of ffprobe's JSON report (the text
/// [`Runner::probe_json`] returns) and flattens it.
///
/// # Errors
/// When `json` is not ffprobe's JSON. `Ok(None)` when there is no stream
/// with that index.
pub fn stream_details(json: &str, index: u32) -> Result<Option<StreamDetails>, ProbeError> {
	#[derive(Deserialize)]
	struct Raw {
		#[serde(default)]
		streams: Vec<Json>,
	}
	let raw: Raw = serde_json::from_str(json).map_err(ProbeError::from)?;
	let Some(stream) = raw.streams.into_iter().find(|stream| {
		stream
			.get("index")
			.and_then(Json::as_u64)
			.is_some_and(|i| u64::from(index) == i)
	}) else {
		return Ok(None);
	};
	let mut fields = Vec::new();
	flatten("", &stream, &mut fields);
	let json = serde_json::to_string_pretty(&stream).map_err(ProbeError::from)?;
	Ok(Some(StreamDetails { fields, json }))
}

/// Appends the scalars under `value` to `out`, keyed as [`StreamDetails::fields`]
/// describes.
fn flatten(key: &str, value: &Json, out: &mut Vec<(String, String)>) {
	match value {
		Json::Object(members) => {
			for (name, member) in members {
				let key = if key.is_empty() {
					name.clone()
				} else {
					format!("{key}.{name}")
				};
				flatten(&key, member, out);
			}
		}
		Json::Array(items) => {
			for (i, item) in items.iter().enumerate() {
				flatten(&format!("{key}[{i}]"), item, out);
			}
		}
		Json::Null => out.push((key.to_owned(), String::new())),
		Json::Bool(b) => out.push((key.to_owned(), b.to_string())),
		Json::Number(n) => out.push((key.to_owned(), n.to_string())),
		Json::String(s) => out.push((key.to_owned(), s.clone())),
	}
}

/// A JSON value whose objects keep their members in document order, so the
/// details read in the order ffprobe printed them (`serde_json::Value`
/// sorts keys).
#[derive(Debug, Clone, PartialEq)]
enum Json {
	Null,
	Bool(bool),
	Number(serde_json::Number),
	String(String),
	Array(Vec<Json>),
	Object(Vec<(String, Json)>),
}

impl Json {
	fn get(&self, name: &str) -> Option<&Json> {
		match self {
			Self::Object(members) => members.iter().find(|(k, _)| k == name).map(|(_, v)| v),
			_ => None,
		}
	}

	fn as_u64(&self) -> Option<u64> {
		match self {
			Self::Number(n) => n.as_u64(),
			Self::String(s) => s.parse().ok(),
			_ => None,
		}
	}
}

impl<'de> Deserialize<'de> for Json {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		struct JsonVisitor;

		impl<'de> Visitor<'de> for JsonVisitor {
			type Value = Json;

			fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				f.write_str("a JSON value")
			}

			fn visit_bool<E: de::Error>(self, v: bool) -> Result<Json, E> {
				Ok(Json::Bool(v))
			}

			fn visit_i64<E: de::Error>(self, v: i64) -> Result<Json, E> {
				Ok(Json::Number(v.into()))
			}

			fn visit_u64<E: de::Error>(self, v: u64) -> Result<Json, E> {
				Ok(Json::Number(v.into()))
			}

			fn visit_f64<E: de::Error>(self, v: f64) -> Result<Json, E> {
				Ok(serde_json::Number::from_f64(v).map_or(Json::Null, Json::Number))
			}

			fn visit_str<E: de::Error>(self, v: &str) -> Result<Json, E> {
				Ok(Json::String(v.to_owned()))
			}

			fn visit_string<E: de::Error>(self, v: String) -> Result<Json, E> {
				Ok(Json::String(v))
			}

			fn visit_none<E: de::Error>(self) -> Result<Json, E> {
				Ok(Json::Null)
			}

			fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
				Ok(Json::Null)
			}

			fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Json, D::Error> {
				Json::deserialize(deserializer)
			}

			fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
				let mut items = Vec::new();
				while let Some(item) = seq.next_element()? {
					items.push(item);
				}
				Ok(Json::Array(items))
			}

			fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
				let mut members = Vec::new();
				while let Some(entry) = map.next_entry()? {
					members.push(entry);
				}
				Ok(Json::Object(members))
			}
		}

		deserializer.deserialize_any(JsonVisitor)
	}
}

impl Serialize for Json {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		match self {
			Self::Null => serializer.serialize_unit(),
			Self::Bool(b) => serializer.serialize_bool(*b),
			Self::Number(n) => n.serialize(serializer),
			Self::String(s) => serializer.serialize_str(s),
			Self::Array(items) => {
				let mut seq = serializer.serialize_seq(Some(items.len()))?;
				for item in items {
					seq.serialize_element(item)?;
				}
				seq.end()
			}
			Self::Object(members) => {
				let mut map = serializer.serialize_map(Some(members.len()))?;
				for (key, value) in members {
					map.serialize_entry(key, value)?;
				}
				map.end()
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe::Disposition;

	fn stream(index: u32, kind: CodecType) -> Stream {
		Stream {
			index,
			codec_type: kind,
			..Stream::default()
		}
	}

	#[test]
	fn packets_win_tags_win_estimates() {
		let mut tagged = stream(0, CodecType::Audio);
		tagged
			.tags
			.insert("NUMBER_OF_BYTES-eng".into(), "16000".into());
		tagged.bit_rate = Some(64_000);
		let estimated = Stream {
			bit_rate: Some(100_000),
			duration: Some(10.0),
			..stream(1, CodecType::Video)
		};
		let unknown = stream(2, CodecType::Subtitle);
		let attachment = Stream {
			extradata_size: Some(8),
			..stream(3, CodecType::Attachment)
		};
		let probe = ProbeResult {
			streams: vec![tagged, estimated, unknown, attachment],
			..ProbeResult::default()
		};
		assert!(needs_measuring(&probe));

		let quick = build(&probe, None);
		let sizes: Vec<_> = quick
			.streams
			.iter()
			.map(|s| (s.size, s.size_method))
			.collect();
		assert_eq!(
			sizes,
			[
				(Some(16_000), SizeMethod::Tags),
				(Some(125_000), SizeMethod::Estimated),
				(None, SizeMethod::Unknown),
				(Some(8), SizeMethod::Extradata),
			]
		);

		let packets = BTreeMap::from([(0, 16_058), (1, 120_000)]);
		let measured = build(&probe, Some(&packets));
		let sizes: Vec<_> = measured
			.streams
			.iter()
			.map(|s| (s.size, s.size_method))
			.collect();
		assert_eq!(
			sizes,
			[
				(Some(16_058), SizeMethod::Packets),
				(Some(120_000), SizeMethod::Packets),
				// Seen by the pass, no packets: zero bytes, not unknown
				(Some(0), SizeMethod::Packets),
				(Some(8), SizeMethod::Extradata),
			]
		);
	}

	#[test]
	fn fully_tagged_files_skip_the_pass() {
		let mut audio = stream(1, CodecType::Audio);
		audio.tags.insert("NUMBER_OF_BYTES".into(), "1".into());
		let attachment = stream(2, CodecType::Attachment);
		let probe = ProbeResult {
			streams: vec![audio, attachment],
			..ProbeResult::default()
		};
		assert!(!needs_measuring(&probe));
		assert!(!needs_measuring(&ProbeResult::default()));
	}

	#[test]
	fn estimate_uses_the_container_duration_when_the_stream_has_none() {
		let probe = ProbeResult {
			streams: vec![Stream {
				bit_rate: Some(8_000),
				..stream(0, CodecType::Audio)
			}],
			format: crate::ffmpeg::probe::Format {
				duration: Some(3.0),
				..crate::ffmpeg::probe::Format::default()
			},
			..ProbeResult::default()
		};
		let report = build(&probe, None);
		assert_eq!(report.streams[0].size, Some(3_000));
		assert_eq!(report.streams[0].duration, Some(3.0));
		assert_eq!(report.container.duration, Some(3.0));
	}

	#[test]
	fn flags_and_tags_are_summarized() {
		let mut s = stream(4, CodecType::Subtitle);
		s.codec_name = Some("subrip".into());
		s.disposition = Disposition {
			default: true,
			forced: true,
			..Disposition::default()
		};
		s.tags.insert("language".into(), "nld".into());
		s.tags.insert("title".into(), "Dutch".into());
		let probe = ProbeResult {
			streams: vec![s, stream(5, CodecType::Unknown)],
			..ProbeResult::default()
		};
		let report = build(&probe, None);
		let row = &report.streams[0];
		assert_eq!(row.codec, "subrip");
		assert_eq!(row.language.as_deref(), Some("nld"));
		assert_eq!(row.title.as_deref(), Some("Dutch"));
		assert_eq!(row.flags, ["default", "forced"]);
		assert_eq!(report.streams[1].codec, "unknown");
		assert_eq!(report.streams[1].flags, Vec::<String>::new());
	}

	#[test]
	fn errors_hide_the_path() {
		let path = Path::new("/media/in/movie.mkv");
		let error = ReportError::from_run(
			RunError::Failed {
				tool: crate::ffmpeg::Tool::Ffprobe,
				status: std::process::ExitStatus::default(),
				log_tail: "/media/in/movie.mkv: Invalid data found when processing input".into(),
			},
			path,
		);
		assert_eq!(
			error.message,
			"the input is not a media file ffmpeg can read"
		);
		assert_eq!(
			error.ffprobe_output,
			"movie.mkv: Invalid data found when processing input"
		);
		let error = ReportError::from_run(
			RunError::TimedOut {
				tool: crate::ffmpeg::Tool::Ffprobe,
				timeout: Duration::from_secs(1),
			},
			path,
		);
		assert_eq!(error.message, "ffprobe did not finish within 1s");
		assert_eq!(error.ffprobe_output, "");
	}

	#[test]
	fn details_keep_ffprobe_order_and_flatten_nesting() {
		let json = r#"{"streams":[
			{"index":0,"codec_type":"video"},
			{"index":"1","codec_name":"aac","sample_rate":"48000","channels":2,"is_avc":true,
			 "x":null,"disposition":{"default":1,"forced":0},"tags":{"language":"eng"},
			 "side_data_list":[{"side_data_type":"Foo"}]}
		]}"#;
		let details = stream_details(json, 1).unwrap().unwrap();
		let fields: Vec<(&str, &str)> = details
			.fields
			.iter()
			.map(|(k, v)| (k.as_str(), v.as_str()))
			.collect();
		assert_eq!(
			fields,
			[
				("index", "1"),
				("codec_name", "aac"),
				("sample_rate", "48000"),
				("channels", "2"),
				("is_avc", "true"),
				("x", ""),
				("disposition.default", "1"),
				("disposition.forced", "0"),
				("tags.language", "eng"),
				("side_data_list[0].side_data_type", "Foo"),
			]
		);
		assert!(
			details.json.starts_with("{\n  \"index\": \"1\","),
			"{}",
			details.json
		);
		assert!(
			details.json.contains("\"side_data_list\": [\n"),
			"{}",
			details.json
		);
		let round_trip: serde_json::Value = serde_json::from_str(&details.json).unwrap();
		assert_eq!(round_trip["tags"]["language"], "eng");
		assert_eq!(round_trip["x"], serde_json::Value::Null);

		assert_eq!(stream_details(json, 7).unwrap(), None);
		assert_eq!(stream_details("{}", 0).unwrap(), None);
		assert!(stream_details("nope", 0).is_err());
	}
}
