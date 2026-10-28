//! Extract every track to its own file, cover art included.
//!
//! [`plan`] resolves a [`Selection`] (the same one a stream copy takes)
//! against a file's streams into an [`Plan`]: one [`Item`] per stream that
//! gets a file, each with a name that carries the stream id, type and
//! language and an extension that fits its codec, plus a [`Skipped`] entry
//! for every selected stream that has no suitable standalone file (fonts
//! and other attachments, data streams, exotic subtitle formats). Nothing
//! broken is written: such a stream is reported, and when it was ticked by
//! hand the plan cannot run until it is unticked.
//!
//! The whole plan is one ffmpeg command with one output per stream, so it
//! runs as one job and reads the input once. Streams are copied, never
//! re-encoded, with two exceptions that only touch tiny data: text
//! subtitles in `mov_text` or `text` form are rewritten as SRT, and cover
//! art that is not already JPEG (a PNG, say) is encoded as a JPEG, which
//! is the point of the feature.
//!
//! | Stream            | File                                              |
//! |-------------------|---------------------------------------------------|
//! | audio             | `.aac` `.mp3` `.flac` `.opus` `.ac3` ... else `.mka` |
//! | video             | `.mkv`                                            |
//! | subtitles         | `.srt` `.ass` `.vtt` `.sup`, bitmap `.mks`        |
//! | attached picture  | `.jpg`                                            |

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::ffmpeg::FfmpegCommand;
use crate::ffmpeg::probe::{CodecType, ProbeResult, Stream};
use crate::streams::{self, Container, Mode, PlanError, Selection};

/// What a stream is, as the output name calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
	/// Moving pictures.
	Video,
	/// Sound.
	Audio,
	/// Subtitles.
	Subtitle,
	/// A picture stored as a video stream with the `attached_pic` flag.
	Cover,
}

impl Kind {
	/// The word in the file name.
	#[must_use]
	pub const fn name(self) -> &'static str {
		match self {
			Self::Video => "video",
			Self::Audio => "audio",
			Self::Subtitle => "subtitle",
			Self::Cover => "cover",
		}
	}
}

impl fmt::Display for Kind {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.name())
	}
}

/// One stream that gets a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
	/// The stream's index, what `-map 0:<index>` refers to.
	pub index: u32,
	/// What the stream is.
	pub kind: Kind,
	/// The codec name, or empty when unknown.
	pub codec: String,
	/// The language tag, when the stream has a usable one.
	pub language: Option<String>,
	/// The file name, without its folder.
	pub file_name: String,
	/// The ffmpeg options between `-map` and the output.
	codec_args: Vec<&'static str>,
}

/// A selected stream that gets no file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
	/// The stream's index.
	pub index: u32,
	/// True when the stream was ticked by hand: the extraction cannot run
	/// until it is unticked. False for streams a mode such as "all" picked
	/// up on its way: they are reported and left out.
	pub blocking: bool,
	/// What the stream is and why it has no file, for the user.
	pub message: String,
}

/// Why nothing can be planned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExtractError {
	/// The selection itself is wrong; see [`PlanError`].
	#[error(transparent)]
	Selection(#[from] PlanError),
	/// Every selected stream lacks a suitable standalone file.
	#[error("none of the selected streams can be written to a file of its own")]
	NothingToExtract,
}

/// The files an extraction writes and the streams it leaves out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
	/// One item per stream with a file, in stream order.
	pub items: Vec<Item>,
	/// The selected streams without one, in stream order.
	pub skipped: Vec<Skipped>,
}

impl Plan {
	/// Whether no blocking [`Skipped`] stands in the way of the job.
	#[must_use]
	pub fn can_run(&self) -> bool {
		!self.skipped.iter().any(|s| s.blocking)
	}

	/// The blocking messages, for a refusal.
	#[must_use]
	pub fn blocking_messages(&self) -> Vec<String> {
		self.skipped
			.iter()
			.filter(|s| s.blocking)
			.map(|s| s.message.clone())
			.collect()
	}

	/// The ffmpeg command that reads `input` once and writes every item
	/// into `folder`.
	#[must_use]
	pub fn command(&self, input: &Path, folder: &Path) -> FfmpegCommand {
		let mut cmd = FfmpegCommand::new().input(input);
		for item in &self.items {
			cmd = cmd
				.option("-map", format!("0:{}", item.index))
				.option("-map_chapters", "-1")
				.args(&item.codec_args)
				.output(folder.join(&item.file_name));
		}
		cmd
	}
}

/// Resolves `selection` against `probe`; `stem` is the input's file name
/// without its extension, which the output names start with.
///
/// # Errors
/// [`ExtractError::Selection`] for a selection that names an unknown stream
/// or keeps nothing, [`ExtractError::NothingToExtract`] when no selected
/// stream can have a file.
pub fn plan(probe: &ProbeResult, selection: &Selection, stem: &str) -> Result<Plan, ExtractError> {
	// The container is irrelevant here; the plan is only used to resolve
	// the selection.
	let resolved = streams::plan(probe, selection, Container::Mkv)?;
	let selected: BTreeSet<u32> = resolved.kept().collect();
	let explicit = selection.mode == Mode::Keep;
	let mut items = Vec::new();
	let mut skipped = Vec::new();
	for stream in probe.streams.iter().filter(|s| selected.contains(&s.index)) {
		match describe(stream) {
			Ok((kind, extension, codec_args)) => {
				items.push(Item {
					index: stream.index,
					kind,
					codec: stream.codec_name.clone().unwrap_or_default(),
					language: language(stream),
					file_name: file_name(stem, stream, kind, extension),
					codec_args,
				});
			}
			Err(message) => skipped.push(Skipped {
				index: stream.index,
				blocking: explicit,
				message: format!("stream {}: {message}", stream.index),
			}),
		}
	}
	if items.is_empty() {
		return Err(ExtractError::NothingToExtract);
	}
	Ok(Plan { items, skipped })
}

/// The usable language tag: letters and digits only, and not "und".
fn language(stream: &Stream) -> Option<String> {
	let tag: String = stream
		.language()?
		.chars()
		.filter(char::is_ascii_alphanumeric)
		.collect();
	(!tag.is_empty() && !tag.eq_ignore_ascii_case("und")).then_some(tag.to_ascii_lowercase())
}

/// `<stem>.s<index>.<kind>[.<language>].<extension>`.
fn file_name(stem: &str, stream: &Stream, kind: Kind, extension: &str) -> String {
	let mut name = format!("{stem}.s{}.{kind}", stream.index);
	if let Some(language) = language(stream) {
		name.push('.');
		name.push_str(&language);
	}
	name.push('.');
	name.push_str(extension);
	name
}

type Described = (Kind, &'static str, Vec<&'static str>);

/// The kind, extension and codec options of a stream, or why it has no
/// file of its own.
fn describe(stream: &Stream) -> Result<Described, String> {
	let codec = stream.codec_name.as_deref().unwrap_or_default();
	let copy = vec!["-c", "copy"];
	match stream.codec_type {
		CodecType::Video if stream.disposition.attached_pic => {
			let codec_args = if codec == "mjpeg" {
				vec!["-c", "copy", "-frames:v", "1", "-update", "1"]
			} else {
				vec![
					"-c:v",
					"mjpeg",
					"-q:v",
					"2",
					"-frames:v",
					"1",
					"-update",
					"1",
				]
			};
			Ok((Kind::Cover, "jpg", codec_args))
		}
		CodecType::Video => Ok((Kind::Video, "mkv", copy)),
		CodecType::Audio => Ok((Kind::Audio, audio_extension(codec), copy)),
		CodecType::Subtitle => {
			subtitle(codec).map(|(extension, args)| (Kind::Subtitle, extension, args))
		}
		CodecType::Attachment => Err(
			"an attachment (usually a font) is not a track; ffmpeg has no standalone file for it"
				.to_owned(),
		),
		CodecType::Data => Err("data streams have no standalone file".to_owned()),
		CodecType::Unknown => Err("the stream type is unknown".to_owned()),
	}
}

/// The raw or native file format of an audio codec; Matroska audio holds
/// any other.
fn audio_extension(codec: &str) -> &'static str {
	match codec {
		"aac" => "aac",
		"mp3" => "mp3",
		"mp2" => "mp2",
		"ac3" => "ac3",
		"eac3" => "eac3",
		"dts" => "dts",
		"truehd" => "thd",
		"flac" => "flac",
		"opus" => "opus",
		"vorbis" => "ogg",
		"alac" => "m4a",
		"wmav1" | "wmav2" => "wma",
		// WAV is little-endian PCM; big-endian and exotic PCM go to Matroska.
		c if c.starts_with("pcm_") && c.ends_with("le") && !c.contains("_f") => "wav",
		_ => "mka",
	}
}

/// The extension and options for a subtitle codec, or why it has none.
fn subtitle(codec: &str) -> Result<(&'static str, Vec<&'static str>), String> {
	let copy = vec!["-c", "copy"];
	match codec {
		"subrip" => Ok(("srt", copy)),
		"ass" | "ssa" => Ok(("ass", copy)),
		"webvtt" => Ok(("vtt", copy)),
		// Text in a form only MP4 or Matroska know is rewritten as SRT.
		"mov_text" | "text" => Ok(("srt", vec!["-c:s", "srt"])),
		"hdmv_pgs_subtitle" => Ok(("sup", copy)),
		"dvd_subtitle" | "dvb_subtitle" => Ok(("mks", copy)),
		"" => Err("the subtitle format is unknown".to_owned()),
		other => Err(format!("{other} subtitles have no standalone file format")),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe::parse;

	/// Video, two audio tracks, three subtitle kinds, a PNG cover, a JPEG
	/// cover and a font.
	fn movie() -> ProbeResult {
		parse(
			r#"{"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264"},
				{"index":1,"codec_type":"audio","codec_name":"aac","tags":{"language":"eng"}},
				{"index":2,"codec_type":"audio","codec_name":"truehd","tags":{"language":"fre"}},
				{"index":3,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"}},
				{"index":4,"codec_type":"subtitle","codec_name":"mov_text","tags":{"language":"und"}},
				{"index":5,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle","tags":{"language":"ger"}},
				{"index":6,"codec_type":"video","codec_name":"png","disposition":{"attached_pic":1}},
				{"index":7,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}},
				{"index":8,"codec_type":"attachment"},
				{"index":9,"codec_type":"audio","codec_name":"pcm_s16be"}
			]}"#,
		)
		.unwrap()
	}

	fn names(plan: &Plan) -> Vec<&str> {
		plan.items.iter().map(|i| i.file_name.as_str()).collect()
	}

	fn command(selection: &Selection) -> FfmpegCommand {
		plan(&movie(), selection, "Movie A (2024)")
			.unwrap()
			.command(
				Path::new("/media/in/Movie A (2024).mkv"),
				Path::new("/media/out"),
			)
	}

	#[test]
	fn every_stream_gets_a_name_with_id_type_and_language() {
		let plan = plan(&movie(), &Selection::all(), "clip").unwrap();
		assert_eq!(
			names(&plan),
			[
				"clip.s0.video.mkv",
				"clip.s1.audio.eng.aac",
				"clip.s2.audio.fre.thd",
				"clip.s3.subtitle.eng.srt",
				"clip.s4.subtitle.srt",
				"clip.s5.subtitle.ger.sup",
				"clip.s6.cover.jpg",
				"clip.s7.cover.jpg",
				"clip.s9.audio.mka",
			]
		);
	}

	#[test]
	fn a_stream_without_a_file_is_reported_not_written() {
		let plan = plan(&movie(), &Selection::all(), "clip").unwrap();
		assert_eq!(plan.skipped.len(), 1);
		assert_eq!(plan.skipped[0].index, 8);
		assert!(!plan.skipped[0].blocking);
		assert!(plan.can_run());
		assert!(
			plan.skipped[0]
				.message
				.starts_with("stream 8: an attachment")
		);
	}

	#[test]
	fn a_ticked_stream_without_a_file_blocks() {
		let plan = plan(&movie(), &Selection::keep([1, 8]), "clip").unwrap();
		assert_eq!(names(&plan), ["clip.s1.audio.eng.aac"]);
		assert!(!plan.can_run());
		assert_eq!(plan.blocking_messages().len(), 1);
	}

	#[test]
	fn selection_errors_and_empty_results() {
		assert_eq!(
			plan(&movie(), &Selection::keep([42]), "x").unwrap_err(),
			ExtractError::Selection(PlanError::UnknownStream(42))
		);
		assert_eq!(
			plan(&movie(), &Selection::keep([8]), "x").unwrap_err(),
			ExtractError::NothingToExtract
		);
		assert_eq!(
			plan(&movie(), &Selection::video_only(), "x")
				.unwrap()
				.items
				.len(),
			1
		);
	}

	#[test]
	fn the_language_is_cleaned() {
		let probe = parse(
			r#"{"streams":[
				{"index":0,"codec_type":"audio","codec_name":"mp3","tags":{"language":"EN/../x"}},
				{"index":1,"codec_type":"audio","codec_name":"mp3","tags":{"language":"und"}}
			]}"#,
		)
		.unwrap();
		let plan = plan(&probe, &Selection::all(), "a").unwrap();
		assert_eq!(names(&plan), ["a.s0.audio.enx.mp3", "a.s1.audio.mp3"]);
	}

	#[test]
	fn all_streams_in_one_command() {
		insta::assert_snapshot!(command(&Selection::all()));
	}

	#[test]
	fn a_subset_in_one_command() {
		insta::assert_snapshot!(command(&Selection::keep([1, 4])));
	}

	#[test]
	fn cover_art_is_written_as_jpeg() {
		insta::assert_snapshot!(command(&Selection::keep([6, 7])));
	}
}
