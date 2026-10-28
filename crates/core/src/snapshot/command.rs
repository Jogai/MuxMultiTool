//! The ffmpeg commands of the snapshot and cover-art features, each with a
//! snapshot test.
//!
//! **Frame** seeks on the original file (an input option, so ffmpeg jumps
//! to the keyframe before the position and decodes up to it, which lands
//! on the exact frame) and writes one picture from the first video stream.
//! The filter turns non-square pixels into square ones, so the picture
//! has the shape the player shows; it does nothing for the usual 1:1.
//! Rotation metadata is applied by ffmpeg itself.
//!
//! **Cover** is a stream copy of everything except the old cover, plus the
//! new picture. The two containers store a cover differently: Matroska as
//! an attachment (`-attach`, with its mimetype and file name as stream
//! metadata), MP4 as an extra video stream with the `attached_pic`
//! disposition. The picture is a file made by [`frame`] beforehand,
//! because the Matroska muxer only writes a real attachment from a file;
//! a picture muxed as a stream would show up as a video track.

use std::path::Path;
use std::time::Duration;

use super::{CoverPlan, ImageFormat, Target};
use crate::ffmpeg::FfmpegCommand;

/// The file name a Matroska cover attachment gets, which is what players
/// look for.
pub const COVER_FILE_NAME: &str = "cover.jpg";

/// The command that writes the frame of `input` at `position` to `output`
/// as `format`. The muxer is forced to `image2`, so the output name's
/// extension does not matter, and no frame at all (a position past the end)
/// is a successful run that writes nothing; see [`super::grab_to_temp`].
#[must_use]
pub fn frame(
	input: &Path,
	output: &Path,
	position: Duration,
	format: ImageFormat,
) -> FfmpegCommand {
	let cmd = FfmpegCommand::new()
		.option("-ss", secs(position))
		.input(input)
		.option("-map", "0:v:0")
		.option("-frames:v", "1")
		.flag("-an")
		.flag("-sn")
		.flag("-dn")
		.option("-vf", "scale=iw*sar:ih,setsar=1")
		.option("-c:v", format.encoder());
	let cmd = match format {
		// Quality 2 is visually lossless. The pixel format is the full-range
		// one the encoder accepts, which also lets it start when there is no
		// frame at the position (ffmpeg then writes nothing and succeeds,
		// as it does for PNG).
		ImageFormat::Jpeg => cmd.option("-q:v", "2").option("-pix_fmt", "yuvj420p"),
		ImageFormat::Png => cmd,
	};
	cmd.option("-f", "image2")
		.option("-update", "1")
		.output(output)
}

/// The command that copies `input` to `output` with the picture at
/// `cover` as its cover art, replacing the old cover. Nothing is
/// re-encoded: the picture is already a JPEG, and it is copied as is.
#[must_use]
pub fn cover(input: &Path, cover: &Path, output: &Path, plan: &CoverPlan) -> FfmpegCommand {
	let mut cmd = FfmpegCommand::new().input(input);
	if plan.target == Target::Mp4 {
		cmd = cmd.input(cover);
	}
	cmd = cmd.option("-map", "0");
	for index in &plan.dropped {
		cmd = cmd.option("-map", format!("-0:{index}"));
	}
	match plan.target {
		Target::Mp4 => {
			cmd = cmd.option("-map", "1:v:0").option("-c", "copy").option(
				&format!("-disposition:v:{}", plan.kept_videos),
				"attached_pic",
			);
		}
		Target::Mkv => {
			let ordinal = plan.kept_attachments;
			cmd = cmd
				.option("-c", "copy")
				.option("-attach", cover)
				.option(&format!("-metadata:s:t:{ordinal}"), "mimetype=image/jpeg")
				.option(
					&format!("-metadata:s:t:{ordinal}"),
					format!("filename={COVER_FILE_NAME}"),
				);
		}
	}
	cmd.output(output)
}

/// Seconds with millisecond precision, as ffmpeg reads them.
fn secs(duration: Duration) -> String {
	format!("{}.{:03}", duration.as_secs(), duration.subsec_millis())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn frame_jpeg_seeks_the_original_before_decoding() {
		let cmd = frame(
			Path::new("/media/in/movie.mkv"),
			Path::new("/tmp/frame.jpg"),
			Duration::from_millis(12_345),
			ImageFormat::Jpeg,
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn frame_png_has_no_quality_option() {
		let cmd = frame(
			Path::new("/media/in/movie.mkv"),
			Path::new("/media/out/movie.png"),
			Duration::from_secs(3600),
			ImageFormat::Png,
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn cover_in_mp4_is_an_attached_picture_stream() {
		let plan = CoverPlan {
			target: Target::Mp4,
			dropped: vec![2],
			kept_videos: 1,
			kept_attachments: 0,
		};
		let cmd = cover(
			Path::new("/media/in/movie.mp4"),
			Path::new("/tmp/cover.jpg"),
			Path::new("/media/out/movie.cover.mp4"),
			&plan,
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn cover_in_mkv_is_an_attachment_after_the_kept_ones() {
		let plan = CoverPlan {
			target: Target::Mkv,
			dropped: vec![5],
			kept_videos: 1,
			kept_attachments: 2,
		};
		let cmd = cover(
			Path::new("/media/in/movie.mkv"),
			Path::new("/tmp/cover.jpg"),
			Path::new("/media/out/movie.cover.mkv"),
			&plan,
		);
		insta::assert_snapshot!(cmd);
	}
}
