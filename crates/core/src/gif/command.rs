//! The ffmpeg commands of the GIF operations, each with a snapshot test.
//!
//! **Video → GIF** is one pass with a two-branch filter graph: the range is
//! decoded once, dropped to the GIF's frame rate and scaled, then split; one
//! branch computes a palette from every frame (`palettegen`), the other
//! quantises the frames with it (`paletteuse`). ffmpeg's GIF encoder alone
//! would use a fixed 256-color palette and dither everything, which looks
//! worse and is larger. `paletteuse`'s `diff_mode=rectangle` stores only the
//! part of each frame that changed, which is where most of the saving on
//! static scenes comes from.
//!
//! **GIF → video** re-encodes with the chosen encoder at a quality-based
//! rate. Both H.264 and VP9/VP8 want 4:2:0 chroma, which needs even
//! dimensions, so the picture is scaled down to the nearest even size (at
//! most one pixel each way).

use std::path::Path;

use super::{Plan, VideoEncoder};
use crate::ffmpeg::FfmpegCommand;

/// The command that makes the GIF of `plan` from `input` at `output`.
///
/// The seek is an input option, so ffmpeg jumps to the keyframe before the
/// start and decodes from there, which is accurate for a transcode. The
/// muxer is forced to `gif` so the output name's extension does not matter.
#[must_use]
pub fn video_to_gif(input: &Path, output: &Path, plan: &Plan) -> FfmpegCommand {
	let graph = format!(
		"[0:v:0]fps={fps},scale={w}:{h}:flags=lanczos,split[a][b];\
		 [a]palettegen=max_colors={colors}[p];\
		 [b][p]paletteuse=dither={dither}:diff_mode=rectangle",
		fps = plan.fps,
		w = plan.width,
		h = plan.height,
		colors = plan.quality.max_colors(),
		dither = plan.quality.dither(),
	);
	FfmpegCommand::new()
		.option("-ss", secs(plan.start))
		.input(input)
		.option("-t", secs(plan.duration))
		.option("-filter_complex", graph)
		.flag("-an")
		.flag("-sn")
		.flag("-dn")
		.option("-loop", if plan.play_once { "-1" } else { "0" })
		.option("-f", "gif")
		.output(output)
}

/// The command that turns the GIF at `input` into a video at `output` with
/// `encoder`. The muxer follows the encoder ([`VideoEncoder::format`]), so
/// the output name's extension does not matter.
#[must_use]
pub fn gif_to_video(input: &Path, output: &Path, encoder: VideoEncoder) -> FfmpegCommand {
	let cmd = FfmpegCommand::new()
		.input(input)
		.option("-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2:flags=lanczos")
		.option("-c:v", encoder.name())
		.option("-pix_fmt", "yuv420p");
	let cmd = match encoder {
		VideoEncoder::X264 => cmd
			.option("-preset", "medium")
			.option("-crf", "23")
			.option("-movflags", "+faststart"),
		// Constant quality: `-b:v 0` makes `-crf` the only rate control.
		VideoEncoder::Vp9 => cmd
			.option("-crf", "32")
			.option("-b:v", "0")
			.option("-row-mt", "1"),
		// VP8 has no pure constant-quality mode; the bitrate is a ceiling.
		VideoEncoder::Vp8 => cmd.option("-crf", "10").option("-b:v", "2M"),
	};
	cmd.flag("-an")
		.option("-f", encoder.format().muxer())
		.output(output)
}

/// Seconds with millisecond precision, as ffmpeg reads them.
fn secs(duration: std::time::Duration) -> String {
	format!("{}.{:03}", duration.as_secs(), duration.subsec_millis())
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use super::*;
	use crate::gif::Quality;

	fn plan(quality: Quality, play_once: bool) -> Plan {
		Plan {
			start: Duration::from_secs(12),
			duration: Duration::from_secs(8),
			fps: 12,
			width: 480,
			height: 200,
			frames: 96,
			estimated_bytes: 0,
			quality,
			play_once,
		}
	}

	#[test]
	fn video_to_gif_balanced_loops_forever() {
		let cmd = video_to_gif(
			Path::new("/media/in/trailer.webm"),
			Path::new("/media/out/trailer.gif"),
			&plan(Quality::Balanced, false),
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn video_to_gif_small_plays_once() {
		let cmd = video_to_gif(
			Path::new("/media/in/trailer.webm"),
			Path::new("/media/out/trailer.gif"),
			&plan(Quality::Small, true),
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn video_to_gif_best() {
		let cmd = video_to_gif(
			Path::new("/media/in/trailer.webm"),
			Path::new("/media/out/trailer.gif"),
			&Plan {
				start: Duration::from_millis(500),
				duration: Duration::from_millis(2_250),
				..plan(Quality::Best, false)
			},
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn gif_to_mp4_with_x264() {
		let cmd = gif_to_video(
			Path::new("/media/in/loop.gif"),
			Path::new("/media/out/loop.mp4"),
			VideoEncoder::X264,
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn gif_to_webm_with_vp9() {
		let cmd = gif_to_video(
			Path::new("/media/in/loop.gif"),
			Path::new("/media/out/loop.webm"),
			VideoEncoder::Vp9,
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn gif_to_webm_with_vp8() {
		let cmd = gif_to_video(
			Path::new("/media/in/loop.gif"),
			Path::new("/media/out/loop.webm"),
			VideoEncoder::Vp8,
		);
		insta::assert_snapshot!(cmd);
	}
}
