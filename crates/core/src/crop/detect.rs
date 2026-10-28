//! Reading what ffmpeg and ffprobe found: the black bars `cropdetect`
//! reports, and the coded size and header crop window of a stream.

use std::path::Path;

use super::{Coded, Crop, Source};

/// What black-bar detection found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detection {
	/// The bars, snapped to the stream's crop unit, in pixels of the
	/// picture as it is shown now.
	pub crop: Crop,
	/// How many frames were looked at.
	pub frames: u32,
}

/// Reads the `cropdetect` lines of an ffmpeg run ([`super::command::detect`])
/// for `source`. The filter prints one line per frame, such as
/// `[Parsed_cropdetect_0 @ 0x…] x1:0 x2:1919 y1:140 y2:939 w:1920 h:800 x:0
/// y:140 pts:… t:… limit:… crop=1920:800:0:140`; with `reset=0` the last
/// line holds the picture found over every frame so far, so only the last
/// one counts.
///
/// `None` when no frame was analysed or every frame was black (the filter
/// then reports an empty picture): there is nothing to suggest.
#[must_use]
pub fn parse_cropdetect<'a>(
	lines: impl IntoIterator<Item = &'a str>,
	source: &Source,
) -> Option<Detection> {
	let mut frames = 0u32;
	let mut last = None;
	for line in lines {
		if let Some(rect) = parse_crop_rect(line) {
			frames += 1;
			last = Some(rect);
		}
	}
	let (w, h, x, y) = last?;
	if w == 0 || h == 0 {
		return None;
	}
	let crop = Crop {
		top: y,
		bottom: source.height.saturating_sub(y.saturating_add(h)),
		left: x,
		right: source.width.saturating_sub(x.saturating_add(w)),
	}
	.snap(source.unit);
	Some(Detection { crop, frames })
}

/// The `crop=W:H:X:Y` at the end of a `cropdetect` line.
fn parse_crop_rect(line: &str) -> Option<(u32, u32, u32, u32)> {
	if !line.contains("cropdetect") {
		return None;
	}
	let rest = line.rsplit("crop=").next()?;
	let mut parts = rest.trim().split(':').map(|p| p.trim().parse::<i64>().ok());
	let mut next = || parts.next().flatten();
	let (w, h, x, y) = (next()?, next()?, next()?, next()?);
	// A fully black frame yields a negative or empty picture.
	let clamp = |v: i64| u32::try_from(v.max(0)).unwrap_or(u32::MAX);
	Some((clamp(w), clamp(h), clamp(x), clamp(y)))
}

/// The ffprobe arguments that print the coded size and header crop window
/// of stream `stream_index` of `path`, for [`parse_coded`]: one decoded
/// frame with the crop left unapplied, as CSV.
#[must_use]
pub fn coded_probe_args(path: &Path, stream_index: u32) -> Vec<String> {
	vec![
		"-hide_banner".to_owned(),
		"-v".to_owned(),
		"error".to_owned(),
		"-apply_cropping".to_owned(),
		"0".to_owned(),
		"-select_streams".to_owned(),
		stream_index.to_string(),
		"-read_intervals".to_owned(),
		"%+#1".to_owned(),
		"-show_entries".to_owned(),
		"frame=width,height,crop_top,crop_bottom,crop_left,crop_right".to_owned(),
		"-of".to_owned(),
		"csv=p=0".to_owned(),
		"-i".to_owned(),
		path.to_string_lossy().into_owned(),
	]
}

/// Reads the first frame line of [`coded_probe_args`]'s output:
/// `width,height,crop_top,crop_bottom,crop_left,crop_right`, e.g.
/// `1920,1088,0,8,0,0` for 1080p. `None` when there is no such line, or
/// the window does not fit the coded size.
#[must_use]
pub fn parse_coded(csv: &str) -> Option<Coded> {
	let line = csv.lines().map(str::trim).find(|l| !l.is_empty())?;
	let mut fields = line.split(',').map(|f| f.trim().parse::<u32>().ok());
	let mut next = || fields.next().flatten();
	let coded = Coded {
		width: next()?,
		height: next()?,
		crop: Crop {
			top: next()?,
			bottom: next()?,
			left: next()?,
			right: next()?,
		},
	};
	let fits = coded.crop.left.checked_add(coded.crop.right)? < coded.width
		&& coded.crop.top.checked_add(coded.crop.bottom)? < coded.height;
	fits.then_some(coded)
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use super::*;
	use crate::crop::{Codec, CropUnit};

	fn source() -> Source {
		Source {
			stream_index: 0,
			video_ordinal: 0,
			codec: Codec::H264,
			width: 1920,
			height: 1080,
			unit: CropUnit { x: 2, y: 2 },
			pix_fmt: "yuv420p".into(),
			duration: Some(Duration::from_secs(60)),
		}
	}

	const LOG: &str = "\
Input #0, matroska,webm, from '/media/in/movie.mkv':
  Stream #0:0: Video: h264 (High), yuv420p(progressive), 1920x1080, 23.98 fps
Stream mapping:
  Stream #0:0 -> #0:0 (h264 (native) -> wrapped_avframe (native))
[Parsed_cropdetect_0 @ 0x55d1] x1:0 x2:1919 y1:142 y2:937 w:1920 h:796 x:0 y:142 pts:1001 t:0.041708 limit:25.500000 crop=1920:796:0:142
[Parsed_cropdetect_0 @ 0x55d1] x1:0 x2:1919 y1:140 y2:939 w:1920 h:800 x:0 y:140 pts:2002 t:0.083417 limit:25.500000 crop=1920:800:0:140
[Parsed_cropdetect_0 @ 0x55d1] x1:0 x2:1919 y1:139 y2:939 w:1920 h:802 x:0 y:139 pts:3003 t:0.125125 limit:25.500000 crop=1920:802:0:139
[out#0/null @ 0x55d2] video:5KiB audio:0KiB subtitle:0KiB other streams:0KiB global headers:0KiB muxing overhead: unknown
";

	#[test]
	fn the_last_line_wins_and_is_snapped() {
		let found = parse_cropdetect(LOG.lines(), &source()).unwrap();
		assert_eq!(found.frames, 3);
		// y:139 h:802 → top 139 → 138, bottom 1080-941 = 139 → 138
		assert_eq!(
			found.crop,
			Crop {
				top: 138,
				bottom: 138,
				left: 0,
				right: 0,
			}
		);
	}

	#[test]
	fn pillarbox_bars_come_out_as_left_and_right() {
		let line = "[Parsed_cropdetect_0 @ 0x1] x1:240 x2:1679 y1:0 y2:1079 w:1440 h:1080 x:240 y:0 pts:1 t:0.04 limit:25.5 crop=1440:1080:240:0";
		let found = parse_cropdetect([line], &source()).unwrap();
		assert_eq!(
			found.crop,
			Crop {
				top: 0,
				bottom: 0,
				left: 240,
				right: 240,
			}
		);
		assert_eq!(found.frames, 1);
	}

	#[test]
	fn no_frames_or_black_frames_find_nothing() {
		assert_eq!(parse_cropdetect(["Stream mapping:"], &source()), None);
		assert_eq!(parse_cropdetect([], &source()), None);
		let black = "[Parsed_cropdetect_0 @ 0x1] x1:1919 x2:0 y1:1079 y2:0 w:-1918 h:-1078 x:0 y:0 pts:1 t:0.04 limit:25.5 crop=0:0:0:0";
		assert_eq!(parse_cropdetect([black], &source()), None);
		let garbage = "[Parsed_cropdetect_0 @ 0x1] crop=a:b:c:d";
		assert_eq!(parse_cropdetect([garbage], &source()), None);
	}

	#[test]
	fn a_picture_larger_than_the_source_cannot_go_negative() {
		let line = "[Parsed_cropdetect_0 @ 0x1] crop=4000:3000:0:0";
		let found = parse_cropdetect([line], &source()).unwrap();
		assert_eq!(found.crop, Crop::default());
	}

	#[test]
	fn coded_probe_reads_one_uncropped_frame() {
		let args = coded_probe_args(Path::new("/media/in/movie.mkv"), 2);
		insta::assert_snapshot!(args.join(" "));
	}

	#[test]
	fn coded_size_and_window_come_from_the_first_line() {
		let coded = parse_coded("1920,1088,0,8,0,0\n1920,1088,0,8,0,0\n").unwrap();
		assert_eq!((coded.width, coded.height), (1920, 1088));
		assert_eq!(
			coded.crop,
			Crop {
				top: 0,
				bottom: 8,
				left: 0,
				right: 0,
			}
		);
		assert_eq!(parse_coded("\n640,368,60,68,0,0\n").unwrap().crop.top, 60);
	}

	#[test]
	fn a_window_that_does_not_fit_or_a_short_line_is_nothing() {
		assert_eq!(parse_coded(""), None);
		assert_eq!(parse_coded("1920,1088,0,8"), None);
		assert_eq!(parse_coded("1920,1088,1000,100,0,0"), None);
		assert_eq!(parse_coded("x,y,0,0,0,0"), None);
	}
}
