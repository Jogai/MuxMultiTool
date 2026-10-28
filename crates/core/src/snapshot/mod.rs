//! Full-resolution snapshots and cover art from a frame.
//!
//! Both come from the original file, never from the preview stream, which
//! is scaled and re-encoded for the player.
//!
//! - A **snapshot** is one decoded frame saved as JPEG or PNG
//!   ([`command::frame`]). [`grab`] returns the bytes for the frame route;
//!   a snapshot job writes the file the user chose.
//! - **Cover art** is a lossless remux that adds a frame as the file's
//!   embedded picture and drops the old one ([`command::cover`]). Only
//!   MKV and MP4 outputs are supported ([`plan_cover`]): they are the
//!   containers with a cover that players show, and the only ones where
//!   the remux is known to work.
//!
//! The commands are in [`command`]; this module holds the decisions that
//! need the probe: which streams are covers now, and whether the target
//! container can hold the rest.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ffmpeg::probe::{CodecType, ProbeResult, Stream};
use crate::ffmpeg::{Limits, RunError, Runner};
use crate::streams::{Compatibility, Container};

pub mod command;

/// How long grabbing one frame may take. Decoding from the keyframe before
/// the position is quick; the margin is for a slow network share.
pub const GRAB_TIMEOUT: Duration = Duration::from_secs(60);

/// The picture formats a snapshot can be saved as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
	/// JPEG, high quality.
	Jpeg,
	/// PNG, lossless.
	Png,
}

impl ImageFormat {
	/// The format for a file name, by its extension (case does not
	/// matter): `jpg`, `jpeg` or `png`.
	#[must_use]
	pub fn from_path(path: &Path) -> Option<Self> {
		Self::from_name(path.extension()?.to_str()?)
	}

	/// The format named `name` (`jpg`, `jpeg`, `png`, any case).
	#[must_use]
	pub fn from_name(name: &str) -> Option<Self> {
		match name.to_ascii_lowercase().as_str() {
			"jpg" | "jpeg" => Some(Self::Jpeg),
			"png" => Some(Self::Png),
			_ => None,
		}
	}

	/// The ffmpeg encoder.
	#[must_use]
	pub const fn encoder(self) -> &'static str {
		match self {
			Self::Jpeg => "mjpeg",
			Self::Png => "png",
		}
	}

	/// The MIME type, for the frame route's `Content-Type`.
	#[must_use]
	pub const fn mime(self) -> &'static str {
		match self {
			Self::Jpeg => "image/jpeg",
			Self::Png => "image/png",
		}
	}

	/// The usual file extension.
	#[must_use]
	pub const fn extension(self) -> &'static str {
		match self {
			Self::Jpeg => "jpg",
			Self::Png => "png",
		}
	}
}

/// Why a frame could not be grabbed.
#[derive(Debug, thiserror::Error)]
pub enum GrabError {
	/// ffmpeg failed or could not be started.
	#[error("ffmpeg could not grab the frame: {0}")]
	Run(#[from] RunError),
	/// ffmpeg ran but wrote nothing: the position is past the last frame, or
	/// the file has no video.
	#[error("there is no video frame at {0:?}")]
	NoFrame(Duration),
	/// The temporary file could not be made or read.
	#[error("cannot use the temporary picture file: {0}")]
	Io(#[from] std::io::Error),
}

/// A picture in a temporary folder that is removed when this is dropped.
#[derive(Debug)]
pub struct TempImage {
	_dir: tempfile::TempDir,
	path: PathBuf,
}

impl TempImage {
	/// The picture file.
	#[must_use]
	pub fn path(&self) -> &Path {
		&self.path
	}
}

/// Grabs the frame of `input` at `position` into a temporary file.
///
/// # Errors
/// See [`GrabError`].
pub async fn grab_to_temp(
	runner: &Runner,
	input: &Path,
	position: Duration,
	format: ImageFormat,
) -> Result<TempImage, GrabError> {
	let dir = tempfile::Builder::new()
		.prefix("muxmultitool-frame-")
		.tempdir()?;
	let path = dir.path().join(format!("frame.{}", format.extension()));
	let cmd = command::frame(input, &path, position, format);
	runner
		.run(&cmd, &Limits::timeout(GRAB_TIMEOUT), None)
		.await?;
	match tokio::fs::metadata(&path).await {
		Ok(meta) if meta.len() > 0 => Ok(TempImage { _dir: dir, path }),
		_ => Err(GrabError::NoFrame(position)),
	}
}

/// Grabs the frame of `input` at `position` and returns the picture's bytes.
///
/// # Errors
/// See [`GrabError`].
pub async fn grab(
	runner: &Runner,
	input: &Path,
	position: Duration,
	format: ImageFormat,
) -> Result<Vec<u8>, GrabError> {
	let image = grab_to_temp(runner, input, position, format).await?;
	Ok(tokio::fs::read(image.path()).await?)
}

/// The container of a cover-art output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
	/// Matroska: the cover is an attachment.
	Mkv,
	/// MP4: the cover is a picture stream flagged `attached_pic`.
	Mp4,
}

impl Target {
	const fn container(self) -> Container {
		match self {
			Self::Mkv => Container::Mkv,
			Self::Mp4 => Container::Mp4,
		}
	}
}

/// What a cover-art remux does to the streams of the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverPlan {
	/// The output container.
	pub target: Target,
	/// Input stream indexes left out: the covers the file has now.
	pub dropped: Vec<u32>,
	/// How many video streams the output keeps, so the new MP4 picture
	/// stream is video stream number `kept_videos`.
	pub kept_videos: u32,
	/// How many attachment streams the output keeps, so the new Matroska
	/// attachment is attachment number `kept_attachments`.
	pub kept_attachments: u32,
}

/// Why a file cannot get cover art in a given output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverError {
	/// The output is not MKV or MP4.
	UnsupportedContainer(String),
	/// The output container cannot hold one of the streams that is copied.
	StreamNotSupported {
		/// The input stream index.
		index: u32,
		/// The container's name.
		container: &'static str,
		/// What kind of stream it is.
		kind: &'static str,
	},
}

impl fmt::Display for CoverError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::UnsupportedContainer(what) => write!(
				f,
				"cover art can only be embedded in MKV and MP4 files, not {what}: other containers \
				 either have no embedded cover or cannot keep it through a lossless remux"
			),
			Self::StreamNotSupported {
				index,
				container,
				kind,
			} => write!(
				f,
				"{container} cannot hold the {kind} stream #{index} of this file, so it cannot be \
				 copied without re-encoding; save the cover art to an MKV file instead"
			),
		}
	}
}

impl std::error::Error for CoverError {}

/// Whether `stream` is a cover now: a picture flagged `attached_pic` (how
/// ffprobe shows an MP4 cover and a Matroska attachment of JPEG or PNG),
/// or an image attachment named `cover.*`.
#[must_use]
pub fn is_cover(stream: &Stream) -> bool {
	if stream.disposition.attached_pic {
		return true;
	}
	if stream.codec_type != CodecType::Attachment {
		return false;
	}
	let tag = |name: &str| {
		stream
			.tags
			.iter()
			.find(|(key, _)| key.eq_ignore_ascii_case(name))
			.map(|(_, value)| value.to_ascii_lowercase())
	};
	tag("mimetype").is_some_and(|m| m.starts_with("image/"))
		&& tag("filename").is_some_and(|n| n.starts_with("cover"))
}

/// Plans the cover-art remux of the file `probe` describes into `output`.
///
/// # Errors
/// See [`CoverError`]. The container comes from the output's extension.
pub fn plan_cover(probe: &ProbeResult, output: &Path) -> Result<CoverPlan, CoverError> {
	let target = match Container::from_extension(output) {
		Some(Container::Mkv) => Target::Mkv,
		Some(Container::Mp4) => Target::Mp4,
		Some(other) => {
			return Err(CoverError::UnsupportedContainer(other.name().to_owned()));
		}
		None => {
			let what = output.extension().map_or_else(
				|| "a file without an extension".to_owned(),
				|e| format!("\".{}\"", e.to_string_lossy()),
			);
			return Err(CoverError::UnsupportedContainer(what));
		}
	};
	let mut plan = CoverPlan {
		target,
		dropped: Vec::new(),
		kept_videos: 0,
		kept_attachments: 0,
	};
	for stream in &probe.streams {
		if is_cover(stream) {
			plan.dropped.push(stream.index);
			continue;
		}
		let kind = match stream.codec_type {
			CodecType::Video => {
				plan.kept_videos += 1;
				"video"
			}
			CodecType::Attachment => {
				plan.kept_attachments += 1;
				"attachment"
			}
			CodecType::Audio => "audio",
			CodecType::Subtitle => "subtitle",
			_ => "data",
		};
		if target.container().holds(stream) == Compatibility::Unsupported {
			return Err(CoverError::StreamNotSupported {
				index: stream.index,
				container: target.container().name(),
				kind,
			});
		}
	}
	Ok(plan)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe;

	fn probe_of(streams: &str) -> ProbeResult {
		probe::parse(&format!(
			r#"{{"format": {{"filename": "x", "nb_streams": 3, "format_name": "x", "format_long_name": "x"}}, "streams": [{streams}]}}"#
		))
		.unwrap()
	}

	const VIDEO: &str = r#"{"index": 0, "codec_type": "video", "codec_name": "h264"}"#;
	const AUDIO: &str = r#"{"index": 1, "codec_type": "audio", "codec_name": "aac"}"#;
	const COVER: &str = r#"{"index": 2, "codec_type": "video", "codec_name": "mjpeg",
		"disposition": {"attached_pic": 1}}"#;
	const FONT: &str = r#"{"index": 3, "codec_type": "attachment", "codec_name": "ttf",
		"tags": {"filename": "a.ttf", "mimetype": "font/ttf"}}"#;
	const PNG_COVER: &str = r#"{"index": 4, "codec_type": "attachment", "codec_name": "png",
		"tags": {"FILENAME": "Cover.png", "mimetype": "image/png"}}"#;

	#[test]
	fn the_old_cover_is_dropped_and_the_rest_counted() {
		let probe = probe_of(&[VIDEO, AUDIO, COVER, FONT, PNG_COVER].join(","));
		let plan = plan_cover(&probe, Path::new("/o/x.mkv")).unwrap();
		assert_eq!(
			plan,
			CoverPlan {
				target: Target::Mkv,
				dropped: vec![2, 4],
				kept_videos: 1,
				kept_attachments: 1,
			}
		);
	}

	#[test]
	fn mp4_and_mkv_are_accepted_by_extension_in_any_case() {
		let probe = probe_of(&[VIDEO, AUDIO].join(","));
		assert_eq!(
			plan_cover(&probe, Path::new("a.M4V")).unwrap().target,
			Target::Mp4
		);
		assert_eq!(
			plan_cover(&probe, Path::new("a.MKV")).unwrap().target,
			Target::Mkv
		);
	}

	#[test]
	fn other_containers_are_rejected_with_the_reason() {
		let probe = probe_of(&[VIDEO, AUDIO].join(","));
		for (name, what) in [("a.webm", "WebM"), ("a.mov", "MOV"), ("a.avi", "\".avi\"")] {
			let error = plan_cover(&probe, Path::new(name)).unwrap_err();
			assert_eq!(error, CoverError::UnsupportedContainer(what.into()));
			assert!(
				error
					.to_string()
					.contains("only be embedded in MKV and MP4")
			);
		}
		let error = plan_cover(&probe, Path::new("noext")).unwrap_err();
		assert!(error.to_string().contains("without an extension"));
	}

	#[test]
	fn mp4_refuses_attachments_it_cannot_hold() {
		let probe = probe_of(&[VIDEO, FONT].join(","));
		let error = plan_cover(&probe, Path::new("a.mp4")).unwrap_err();
		assert_eq!(
			error,
			CoverError::StreamNotSupported {
				index: 3,
				container: "MP4",
				kind: "attachment"
			}
		);
		assert!(error.to_string().contains("MP4 cannot hold"));
	}

	#[test]
	fn image_formats_follow_the_extension() {
		assert_eq!(
			ImageFormat::from_path(Path::new("a.JPEG")),
			Some(ImageFormat::Jpeg)
		);
		assert_eq!(
			ImageFormat::from_path(Path::new("a.png")),
			Some(ImageFormat::Png)
		);
		assert_eq!(ImageFormat::from_path(Path::new("a.gif")), None);
		assert_eq!(ImageFormat::from_name("jpg"), Some(ImageFormat::Jpeg));
	}
}
