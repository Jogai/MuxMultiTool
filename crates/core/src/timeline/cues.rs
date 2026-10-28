//! Subtitle cues: which streams can be shown, the ffmpeg command that
//! converts one to WebVTT, and the WebVTT parser.
//!
//! Text subtitles of any codec (SubRip, ASS/SSA, MOV text, WebVTT, ...) are
//! converted by ffmpeg to WebVTT, the one format with a trivial grammar,
//! into a file that is read while ffmpeg writes it (`-flush_packets 1`
//! makes every cue reach the file at once). The cues keep their text and
//! lose their styling: the timeline shows words, not typesetting.
//!
//! Image-based subtitles (PGS, VobSub, DVB, XSUB) are pictures without
//! text; they are refused before ffmpeg starts.

use std::path::Path;

use crate::ffmpeg::FfmpegCommand;

/// One subtitle.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
	/// When it appears, in seconds.
	pub start: f64,
	/// When it disappears, in seconds.
	pub end: f64,
	/// Plain text, lines separated by `\n`, tags removed.
	pub text: String,
}

/// Subtitle codecs that carry pictures, not text.
const IMAGE_CODECS: &[&str] = &["hdmv_pgs_subtitle", "dvd_subtitle", "dvb_subtitle", "xsub"];

/// Whether a subtitle codec (ffprobe's `codec_name`) is image-based.
#[must_use]
pub fn is_image_codec(codec_name: &str) -> bool {
	IMAGE_CODECS.contains(&codec_name)
}

/// The ffmpeg command that converts subtitle stream `stream_index` of
/// `source` to WebVTT at `output`, flushing every cue as it goes.
#[must_use]
pub fn command(source: &Path, stream_index: u32, output: &Path) -> FfmpegCommand {
	FfmpegCommand::new()
		.input(source)
		.option("-map", format!("0:{stream_index}"))
		.option("-f", "webvtt")
		.option("-flush_packets", "1")
		.flag("-y")
		.output(output)
}

/// The cues in WebVTT `text`, in file order.
///
/// With `complete` false, the text is a file still being written and the
/// last block, which may be cut off, is left out unless it ends with a
/// blank line. The header, `NOTE`, `STYLE` and `REGION` blocks, cue
/// identifiers and cue settings are skipped; a block whose timing line
/// cannot be read is skipped as well.
#[must_use]
pub fn parse_webvtt(text: &str, complete: bool) -> Vec<Cue> {
	let text = text.replace("\r\n", "\n").replace('\r', "\n");
	let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
	let mut blocks: Vec<&str> = text.split("\n\n").collect();
	// The last piece of an unfinished file is complete only when the text
	// ended with "\n\n", in which case the split leaves an empty piece.
	if !complete && blocks.last().is_some_and(|last| !last.trim().is_empty()) {
		blocks.pop();
	}
	blocks.into_iter().filter_map(parse_block).collect()
}

fn parse_block(block: &str) -> Option<Cue> {
	let mut lines = block.lines().filter(|line| !line.trim().is_empty());
	let first = lines.next()?;
	let (timing, text_lines) = if first.contains("-->") {
		(first, lines)
	} else {
		if first.starts_with("WEBVTT")
			|| first.starts_with("NOTE")
			|| first.starts_with("STYLE")
			|| first.starts_with("REGION")
		{
			return None;
		}
		// A cue identifier: the timing line follows.
		let timing = lines.next()?;
		if !timing.contains("-->") {
			return None;
		}
		(timing, lines)
	};
	let (start, rest) = timing.split_once("-->")?;
	let end = rest.split_whitespace().next()?;
	let start = timestamp(start.trim())?;
	let end = timestamp(end)?;
	let text = text_lines.map(plain_text).collect::<Vec<_>>().join("\n");
	Some(Cue { start, end, text })
}

/// `[HH:]MM:SS.mmm` in seconds. The seconds must have the three decimals
/// WebVTT prescribes, so a timing line cut off mid-write is not a cue.
fn timestamp(text: &str) -> Option<f64> {
	let (hm, seconds) = text.rsplit_once(':')?;
	let (whole, millis) = seconds.split_once('.')?;
	if whole.len() != 2 || millis.len() != 3 {
		return None;
	}
	let seconds = f64::from(digits(whole)?) + f64::from(digits(millis)?) / 1000.0;
	let mut hours_minutes = hm.rsplit(':');
	let minutes = f64::from(digits(hours_minutes.next()?)?);
	let hours = match hours_minutes.next() {
		Some(hours) => f64::from(digits(hours)?),
		None => 0.0,
	};
	if hours_minutes.next().is_some() {
		return None;
	}
	Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

/// A field of one or more ASCII digits.
fn digits(text: &str) -> Option<u32> {
	(!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
		.then(|| text.parse().ok())
		.flatten()
}

/// The line without tags (`<i>`, `<c.class>`, `<v Name>`, timestamps) and
/// with the character references WebVTT defines decoded.
fn plain_text(line: &str) -> String {
	let mut out = String::with_capacity(line.len());
	let mut rest = line;
	while let Some(open) = rest.find('<') {
		out.push_str(&rest[..open]);
		if let Some(close) = rest[open..].find('>') {
			rest = &rest[open + close + 1..];
		} else {
			// An unclosed '<' is text
			out.push_str(&rest[open..]);
			rest = "";
		}
	}
	out.push_str(rest);
	decode_references(&out)
}

fn decode_references(text: &str) -> String {
	if !text.contains('&') {
		return text.to_owned();
	}
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(amp) = rest.find('&') {
		out.push_str(&rest[..amp]);
		rest = &rest[amp..];
		let replacement = rest.find(';').and_then(|semi| {
			let decoded = match &rest[..=semi] {
				"&amp;" => "&",
				"&lt;" => "<",
				"&gt;" => ">",
				"&quot;" => "\"",
				"&apos;" => "'",
				"&nbsp;" => "\u{a0}",
				"&lrm;" => "\u{200e}",
				"&rlm;" => "\u{200f}",
				_ => return None,
			};
			Some((decoded, semi + 1))
		});
		if let Some((decoded, len)) = replacement {
			out.push_str(decoded);
			rest = &rest[len..];
		} else {
			out.push('&');
			rest = &rest[1..];
		}
	}
	out.push_str(rest);
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	const FFMPEG_OUTPUT: &str = "WEBVTT\n\n00:00.500 --> 00:01.500\nHello <i>there</i>\n\n\
		00:02.000 --> 00:03.000\nSecond\nline two\n";

	fn cue(start: f64, end: f64, text: &str) -> Cue {
		Cue {
			start,
			end,
			text: text.to_owned(),
		}
	}

	#[test]
	fn parses_what_ffmpeg_writes() {
		assert_eq!(
			parse_webvtt(FFMPEG_OUTPUT, true),
			[
				cue(0.5, 1.5, "Hello there"),
				cue(2.0, 3.0, "Second\nline two")
			]
		);
	}

	#[test]
	fn an_unfinished_file_keeps_only_terminated_blocks() {
		assert_eq!(
			parse_webvtt(FFMPEG_OUTPUT, false),
			[cue(0.5, 1.5, "Hello there")]
		);
		// A blank line after the last cue makes it complete
		let terminated = format!("{FFMPEG_OUTPUT}\n");
		assert_eq!(parse_webvtt(&terminated, false).len(), 2);
		// Cut in the middle of a timing line
		assert_eq!(parse_webvtt("WEBVTT\n\n00:00.500 --> 00:0", false), []);
		assert_eq!(parse_webvtt("WEBVTT\n\n00:00.500 --> 00:0", true), []);
		assert_eq!(parse_webvtt("", false), []);
		assert_eq!(parse_webvtt("WEBVTT", true), []);
	}

	#[test]
	fn identifiers_hours_settings_notes_and_crlf() {
		let text = "\u{feff}WEBVTT - with a title\r\n\r\nNOTE a comment\r\nover two lines\r\n\r\n\
			STYLE\r\n::cue { color: red }\r\n\r\n\
			intro\r\n01:02:03.250 --> 01:02:04.000 line:90% align:start\r\n<v Bob>Hi <b>all</b>\r\n\r\n\
			2\r\n00:00:05.000 --> 00:00:06.000\r\n<00:00:05.500><c.yellow>karaoke</c>\r\n\r\n\
			garbage --> here\r\nskipped\r\n\r\n\
			00:00:07.000 --> 00:00:08.000\r\nTom &amp; Jerry &lt;3 &unknown; & co&nbsp;\r\n";
		assert_eq!(
			parse_webvtt(text, true),
			[
				cue(3723.25, 3724.0, "Hi all"),
				cue(5.0, 6.0, "karaoke"),
				cue(7.0, 8.0, "Tom & Jerry <3 &unknown; & co\u{a0}"),
			]
		);
	}

	#[test]
	fn timestamps() {
		assert_eq!(timestamp("00:01.500"), Some(1.5));
		assert_eq!(timestamp("12:34:56.789"), Some(45_296.789));
		assert_eq!(timestamp("1:02.000"), Some(62.0));
		assert_eq!(timestamp("1:2:3:04.000"), None);
		assert_eq!(timestamp("x:04.000"), None);
		assert_eq!(timestamp("5"), None);
		assert_eq!(timestamp("00:-1.000"), None);
		assert_eq!(timestamp("00:01.5"), None);
		assert_eq!(timestamp("00:1.500"), None);
		assert_eq!(timestamp("00:0"), None);
	}

	#[test]
	fn tags_are_stripped_and_a_lone_angle_bracket_stays() {
		assert_eq!(plain_text("a <i>b</i> c"), "a b c");
		assert_eq!(plain_text("1 < 2"), "1 < 2");
		assert_eq!(plain_text("<b>bold"), "bold");
		assert_eq!(plain_text("x <unclosed"), "x <unclosed");
	}

	#[test]
	fn image_codecs() {
		assert!(is_image_codec("hdmv_pgs_subtitle"));
		assert!(is_image_codec("dvd_subtitle"));
		assert!(!is_image_codec("subrip"));
		assert!(!is_image_codec("ass"));
		assert!(!is_image_codec("mov_text"));
	}

	#[test]
	fn cue_command_line() {
		insta::assert_snapshot!(command(
			Path::new("/media/in/movie.mkv"),
			3,
			Path::new("/tmp/mmt/cues-3.vtt")
		));
	}
}
