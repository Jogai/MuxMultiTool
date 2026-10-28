//! The ffmpeg command of a combine, with a snapshot test per use case.
//!
//! The line is the one a user would type: one `-i` per used source, in the
//! order the tracks first use them, each preceded by `-itsoffset` when the
//! source is shifted; one `-map <input>:<stream>` per track, in output
//! order; `-c copy`; then the edits as `-metadata:s:<n>` and
//! `-disposition:<n>` options on the output track `n` (the stream
//! specifier of `-metadata` takes the `s:` prefix, that of `-disposition`
//! does not: there `s` would mean the subtitle streams); and the output.
//! The container follows the output's extension, which is also how
//! [`Container`](crate::streams::Container) was chosen, so no `-f` is
//! needed. Global metadata and chapters come from the first input, as
//! ffmpeg does by default; the stream tags and flags come with each stream.

use std::path::Path;

use super::Plan;
use crate::ffmpeg::FfmpegCommand;

/// The command that copies the tracks of `plan` into `output` without
/// re-encoding.
#[must_use]
pub fn combine(plan: &Plan, output: &Path) -> FfmpegCommand {
	let mut cmd = FfmpegCommand::new();
	for source in plan.inputs() {
		if source.offset_secs != 0.0 {
			cmd = cmd.option("-itsoffset", seconds(source.offset_secs));
		}
		cmd = cmd.input(&source.path);
	}
	for track in &plan.tracks {
		cmd = cmd.option("-map", format!("{}:{}", track.input, track.stream));
	}
	cmd = cmd.option("-c", "copy");
	for track in &plan.tracks {
		let n = track.position;
		for (key, value) in &track.edits.metadata {
			cmd = cmd.option(&format!("-metadata:s:{n}"), format!("{key}={value}"));
		}
		if let Some(flags) = &track.edits.disposition {
			let value = if flags.is_empty() {
				"0".to_owned()
			} else {
				flags.join("+")
			};
			cmd = cmd.option(&format!("-disposition:{n}"), value);
		}
	}
	cmd.output(output)
}

/// Seconds for `-itsoffset`, to the millisecond and without trailing
/// zeros: `2.5`, `-0.04`, `3`.
fn seconds(secs: f64) -> String {
	let text = format!("{secs:.3}");
	let text = text.trim_end_matches('0').trim_end_matches('.');
	if text == "-0" {
		"0".to_owned()
	} else {
		text.to_owned()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::combine::{Source, Track, plan};
	use crate::ffmpeg::probe::parse;
	use crate::streams::Container;

	/// A: an MKV with video and English audio.
	fn movie() -> Source {
		let probe = parse(
			r#"{"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264"},
				{"index":1,"codec_type":"audio","codec_name":"aac","tags":{"language":"eng"}}
			]}"#,
		)
		.unwrap();
		Source::from_probe(Path::new("/media/in/Movie (2024).mkv"), &probe)
	}

	/// B: another MKV of the same movie, with an English subtitle track.
	fn other_release() -> Source {
		let probe = parse(
			r#"{"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264"},
				{"index":1,"codec_type":"audio","codec_name":"ac3","tags":{"language":"fre"}},
				{"index":2,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng","title":"English"},"disposition":{"default":1}}
			]}"#,
		)
		.unwrap();
		Source::from_probe(Path::new("/media/in/Movie (2024) [other].mkv"), &probe)
	}

	/// B: the Blu-ray, with `TrueHD`, AC-3 and PGS subtitles.
	fn bluray() -> Source {
		let probe = parse(
			r#"{"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264"},
				{"index":1,"codec_type":"audio","codec_name":"truehd","tags":{"language":"eng"},"disposition":{"default":1}},
				{"index":2,"codec_type":"audio","codec_name":"ac3","tags":{"language":"eng"}},
				{"index":6,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle","tags":{"language":"eng"}}
			]}"#,
		)
		.unwrap();
		Source::from_probe(Path::new("/media/in/Movie.2024.BluRay.m2ts"), &probe)
	}

	/// C: a SubRip file.
	fn srt() -> Source {
		let probe =
			parse(r#"{"streams":[{"index":0,"codec_type":"subtitle","codec_name":"subrip"}]}"#)
				.unwrap();
		Source::from_probe(Path::new("/media/in/Movie.2024.eng.srt"), &probe)
	}

	fn command(sources: &[Source], tracks: &[Track], output: &str) -> FfmpegCommand {
		let output = Path::new(output);
		let container = Container::from_extension(output).unwrap();
		plan(sources, tracks, container).unwrap().command(output)
	}

	#[test]
	fn subtitles_from_another_file_are_added_as_they_are() {
		insta::assert_snapshot!(command(
			&[movie(), other_release()],
			&[Track::new(0, 0), Track::new(0, 1), Track::new(1, 2)],
			"/media/out/Movie (2024).subs.mkv"
		));
	}

	#[test]
	fn bdrip_video_with_bluray_audio_and_subs_sets_the_track_metadata() {
		insta::assert_snapshot!(command(
			&[movie(), bluray().with_offset(2.0), srt()],
			&[
				Track::new(0, 0),
				Track::new(1, 1).title("Atmos"),
				Track::new(1, 2).title("Compatibility").as_default(false),
				Track::new(2, 0)
					.language("eng")
					.title("English")
					.as_default(true),
				Track::new(1, 6),
			],
			"/media/out/Movie (2024).mkv"
		));
	}

	#[test]
	fn a_cleared_title_and_flags_are_written_as_empty() {
		insta::assert_snapshot!(command(
			&[movie(), other_release()],
			&[
				Track::new(1, 0),
				Track::new(0, 1).language("").as_forced(false),
				Track::new(1, 2).title("").as_default(false),
			],
			"/media/out/Movie (2024).clean.mkv"
		));
	}

	#[test]
	fn only_the_used_sources_are_inputs() {
		insta::assert_snapshot!(command(
			&[movie(), bluray().with_offset(-0.5), srt()],
			&[Track::new(2, 0), Track::new(0, 0)],
			"/media/out/Movie (2024).mkv"
		));
	}

	#[test]
	fn offsets_are_written_to_the_millisecond() {
		assert_eq!(seconds(2.0), "2");
		assert_eq!(seconds(2.5), "2.5");
		assert_eq!(seconds(-0.04), "-0.04");
		assert_eq!(seconds(0.0004), "0");
		assert_eq!(seconds(-0.0004), "0");
		assert_eq!(seconds(1.2345), "1.234");
	}
}
