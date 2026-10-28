//! The ffmpeg command of a stream copy, with a snapshot test per mode.
//!
//! The line is the one a user would type: the input, one `-map 0:<index>`
//! per kept stream (or `-map 0` when every stream is kept, the remux
//! idiom), `-c copy`, and the output. The container follows the output's
//! extension, which is also how [`Container`](super::Container) was chosen,
//! so no `-f` is needed. Metadata and chapters are copied by default.

use std::path::Path;

use super::Plan;
use crate::ffmpeg::FfmpegCommand;

/// The command that copies the kept streams of `plan` from `input` into
/// `output` without re-encoding.
#[must_use]
pub fn copy_streams(input: &Path, output: &Path, plan: &Plan) -> FfmpegCommand {
	let mut cmd = FfmpegCommand::new().input(input);
	if plan.keeps_everything() {
		cmd = cmd.option("-map", "0");
	} else {
		for index in plan.kept() {
			cmd = cmd.option("-map", format!("0:{index}"));
		}
	}
	cmd.option("-c", "copy").output(output)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe::parse;
	use crate::streams::{Container, Selection, plan};

	/// Video, English and French audio, two subtitles and a font.
	fn movie() -> crate::ffmpeg::probe::ProbeResult {
		parse(
			r#"{"streams":[
				{"index":0,"codec_type":"video","codec_name":"hevc"},
				{"index":1,"codec_type":"audio","codec_name":"aac","tags":{"language":"eng"}},
				{"index":2,"codec_type":"audio","codec_name":"dts","tags":{"language":"fre"}},
				{"index":3,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"}},
				{"index":4,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"fre"}},
				{"index":5,"codec_type":"attachment"}
			]}"#,
		)
		.unwrap()
	}

	fn command(selection: &Selection, output: &str) -> FfmpegCommand {
		let output = Path::new(output);
		let container = Container::from_extension(output).unwrap();
		plan(&movie(), selection, container)
			.unwrap()
			.command(Path::new("/media/in/Movie A (2024).mkv"), output)
	}

	#[test]
	fn remux_copies_every_stream() {
		insta::assert_snapshot!(command(&Selection::all(), "/media/out/Movie A (2024).mkv"));
	}

	#[test]
	fn keep_maps_only_the_ticked_streams() {
		insta::assert_snapshot!(command(
			&Selection::keep([0, 1, 3]),
			"/media/out/Movie A (2024).eng.mkv"
		));
	}

	#[test]
	fn drop_maps_everything_else() {
		insta::assert_snapshot!(command(
			&Selection::drop([2, 4]),
			"/media/out/Movie A (2024).mkv"
		));
	}

	#[test]
	fn video_only_maps_the_video_stream() {
		insta::assert_snapshot!(command(
			&Selection::video_only(),
			"/media/out/Movie A (2024).video.mp4"
		));
	}
}
