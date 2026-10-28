//! The ffmpeg commands of the crop feature, each with a snapshot test.
//!
//! **Crop** is a remux (`-map 0 -c copy`) with the codec's metadata
//! bitstream filter on the video stream, which rewrites the crop window in
//! every sequence header it passes, the container's extradata included.
//! The offsets given to the filter are the header's, see
//! [`Plan::header_crop`].
//!
//! **Detect** decodes keyframes only (`-skip_frame nokey`), starting at the
//! keyframe before the asked position (`-noaccurate_seek`, so that
//! keyframe is kept rather than dropped as "before the seek point"), and
//! runs `cropdetect` over [`DETECT_FRAMES`] of them without resetting, so
//! the result is the union of the picture found in all of them: a dark
//! frame cannot shrink it. Keyframes are cheap to decode alone and sit a
//! few seconds apart, so twelve of them cover a scene or two.

use std::path::Path;
use std::time::Duration;

use super::{Coded, DETECT_FRAMES, Plan, Source};
use crate::ffmpeg::FfmpegCommand;

/// The command that writes `input` to `output` with the crop window of
/// `plan` in the video stream's header. Every stream is copied.
#[must_use]
pub fn crop(input: &Path, output: &Path, plan: &Plan, coded: &Coded) -> FfmpegCommand {
	let window = plan.header_crop(coded);
	let filter = format!(
		"{}=crop_left={}:crop_right={}:crop_top={}:crop_bottom={}",
		plan.source.codec.bitstream_filter(),
		window.left,
		window.right,
		window.top,
		window.bottom,
	);
	FfmpegCommand::new()
		.input(input)
		.option("-map", "0")
		.option("-c", "copy")
		.option(&format!("-bsf:v:{}", plan.source.video_ordinal), filter)
		.output(output)
}

/// The command that runs `cropdetect` over the keyframes of `source` in
/// `input` from `position` on and writes nothing. The findings are on
/// stderr, one line per frame; see [`super::detect::parse_cropdetect`].
///
/// The black threshold is relative (`0.1` of full scale), so it means the
/// same for 8-bit and 10-bit video.
#[must_use]
pub fn detect(input: &Path, source: &Source, position: Duration) -> FfmpegCommand {
	FfmpegCommand::new()
		.flag("-noaccurate_seek")
		.option("-skip_frame", "nokey")
		.option("-ss", secs(position))
		.input(input)
		.option("-map", format!("0:{}", source.stream_index))
		.option("-frames:v", DETECT_FRAMES.to_string())
		.option("-vf", "cropdetect=limit=0.1:round=2:reset=0:skip=0")
		.option("-f", "null")
		.output("-")
}

/// Seconds with millisecond precision, as ffmpeg reads them.
fn secs(duration: Duration) -> String {
	format!("{}.{:03}", duration.as_secs(), duration.subsec_millis())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::crop::{Codec, Crop, CropUnit, plan};

	fn source(codec: Codec) -> Source {
		Source {
			stream_index: 1,
			video_ordinal: 0,
			codec,
			width: 1920,
			height: 1080,
			unit: CropUnit { x: 2, y: 2 },
			pix_fmt: "yuv420p".into(),
			duration: Some(Duration::from_secs(7200)),
		}
	}

	/// 1080p as encoders store it: 1088 lines with 8 cropped off the bottom.
	fn coded_1080p() -> Coded {
		Coded {
			width: 1920,
			height: 1088,
			crop: Crop {
				bottom: 8,
				..Crop::default()
			},
		}
	}

	fn bars() -> Crop {
		Crop {
			top: 140,
			bottom: 140,
			left: 0,
			right: 0,
		}
	}

	#[test]
	fn crop_h264_adds_the_request_to_the_header_window() {
		let plan = plan(&source(Codec::H264), bars()).unwrap();
		let cmd = crop(
			Path::new("/media/in/movie.mkv"),
			Path::new("/media/out/movie.cropped.mkv"),
			&plan,
			&coded_1080p(),
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn crop_hevc_on_the_second_video_stream() {
		let plan = plan(
			&Source {
				video_ordinal: 1,
				stream_index: 2,
				..source(Codec::Hevc)
			},
			Crop {
				top: 0,
				bottom: 0,
				left: 240,
				right: 241,
			},
		)
		.unwrap();
		assert_eq!(plan.crop.right, 240, "snapped");
		let cmd = crop(
			Path::new("/media/in/movie.mp4"),
			Path::new("/media/out/movie.cropped.mp4"),
			&plan,
			&coded_1080p(),
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn detect_scans_keyframes_from_the_position() {
		let cmd = detect(
			Path::new("/media/in/movie.mkv"),
			&source(Codec::Hevc),
			Duration::from_millis(754_500),
		);
		insta::assert_snapshot!(cmd);
	}
}
