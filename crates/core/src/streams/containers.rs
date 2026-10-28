//! What each target container can hold on a stream copy.
//!
//! ffmpeg only finds out at `-c copy` time that a muxer has no tag for a
//! codec, and then the job fails at once with "codec not currently
//! supported in container". The table here says so before anything runs.
//! It is deliberately conservative: a combination is [`Unsupported`] only
//! when ffmpeg is known to refuse it, [`Supported`] when it is known to
//! work, and [`Uncertain`] otherwise, so an exotic codec gets a warning
//! rather than a refusal.
//!
//! The rules were checked against an ffmpeg 8 build (2026-10); the comments
//! on each container name the muxer's own message where it has one.
//!
//! [`Unsupported`]: Compatibility::Unsupported
//! [`Supported`]: Compatibility::Supported
//! [`Uncertain`]: Compatibility::Uncertain

use std::fmt;
use std::path::Path;

use crate::ffmpeg::probe::{CodecType, Stream};

/// A target container of a stream copy, chosen by the output's extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Container {
	/// Matroska (`.mkv`, `.mka`, `.mks`): holds nearly everything.
	Mkv,
	/// MP4 (`.mp4`, `.m4v`, `.m4a`): the ISO base media file format with
	/// the MP4 codec tags.
	Mp4,
	/// `QuickTime` (`.mov`): the same family with Apple's tags, which cover
	/// `ProRes` and PCM but not Opus or FLAC.
	Mov,
	/// `WebM` (`.webm`): VP8/VP9/AV1, Vorbis/Opus and `WebVTT` only.
	Webm,
}

/// Every container, in the order the messages list them.
pub const CONTAINERS: [Container; 4] = [
	Container::Mkv,
	Container::Mp4,
	Container::Mov,
	Container::Webm,
];

/// Whether a container can hold a stream on a copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
	/// ffmpeg is known to write it.
	Supported,
	/// ffmpeg is known to refuse it; the job would fail at once.
	Unsupported,
	/// Not in the table either way; ffmpeg decides when the job runs.
	Uncertain,
}

impl Container {
	/// The container for an output file name, by its extension (case does
	/// not matter), or `None` for an extension no container claims.
	#[must_use]
	pub fn from_extension(path: &Path) -> Option<Self> {
		let extension = path.extension()?.to_str()?.to_ascii_lowercase();
		CONTAINERS
			.into_iter()
			.find(|c| c.extensions().contains(&extension.as_str()))
	}

	/// The name shown to the user.
	#[must_use]
	pub const fn name(self) -> &'static str {
		match self {
			Self::Mkv => "MKV",
			Self::Mp4 => "MP4",
			Self::Mov => "MOV",
			Self::Webm => "WebM",
		}
	}

	/// The file extensions that select this container, the usual one first.
	#[must_use]
	pub const fn extensions(self) -> &'static [&'static str] {
		match self {
			Self::Mkv => &["mkv", "mka", "mks"],
			Self::Mp4 => &["mp4", "m4v", "m4a"],
			Self::Mov => &["mov"],
			Self::Webm => &["webm"],
		}
	}

	/// Whether this container can hold `stream` on a copy.
	#[must_use]
	pub fn holds(self, stream: &Stream) -> Compatibility {
		use Compatibility::{Supported, Unsupported};
		let codec = stream.codec_name.as_deref().unwrap_or_default();
		let tag = stream.codec_tag_string.as_deref().unwrap_or_default();
		match (self, stream.codec_type) {
			// "Only audio, video, and subtitles are supported for Matroska":
			// attachments are the exception the muxer makes itself.
			(Self::Mkv, CodecType::Attachment) => Supported,
			// Matroska has a generic tag for any video or audio codec.
			(Self::Mkv, CodecType::Video | CodecType::Audio) => known(codec),
			// "Subtitle codec … is not supported": the muxer's own list.
			(Self::Mkv, CodecType::Subtitle) => listed(codec, MKV_SUBTITLES),

			// MP4 and MOV share a muxer with two tag tables. A QuickTime
			// timecode track is a data stream MOV keeps and MP4 has no tag for.
			(Self::Mov, CodecType::Data) if tag == "tmcd" => Supported,
			(Self::Mp4, CodecType::Video) => one_of(codec, MP4_VIDEO, MP4_VIDEO_REFUSED),
			(Self::Mov, CodecType::Video) => one_of(codec, MOV_VIDEO, MOV_VIDEO_REFUSED),
			(Self::Mp4, CodecType::Audio) => one_of(codec, MP4_AUDIO, MP4_AUDIO_REFUSED),
			(Self::Mov, CodecType::Audio) if codec.starts_with("pcm_") => Supported,
			(Self::Mov, CodecType::Audio) => one_of(codec, MOV_AUDIO, MOV_AUDIO_REFUSED),
			(Self::Mp4, CodecType::Subtitle) => listed(codec, MP4_SUBTITLES),
			(Self::Mov, CodecType::Subtitle) => listed(codec, MOV_SUBTITLES),

			// "Only VP8 or VP9 or AV1 video and Vorbis or Opus audio and
			// WebVTT subtitles are supported for WebM."
			(Self::Webm, CodecType::Video) => listed(codec, &["vp8", "vp9", "av1"]),
			(Self::Webm, CodecType::Audio) => listed(codec, &["vorbis", "opus"]),
			(Self::Webm, CodecType::Subtitle) => listed(codec, &["webvtt"]),

			// Data streams and unknown types everywhere else, and attachments
			// outside Matroska.
			(Self::Mkv, CodecType::Data | CodecType::Unknown)
			| (
				Self::Mp4 | Self::Mov | Self::Webm,
				CodecType::Attachment | CodecType::Data | CodecType::Unknown,
			) => Unsupported,
		}
	}

	/// How the message names streams of `kind` in this container, e.g.
	/// "SubRip subtitles".
	pub(crate) fn describe(stream: &Stream) -> String {
		let kind = match stream.codec_type {
			CodecType::Video => "video",
			CodecType::Audio => "audio",
			CodecType::Subtitle => "subtitles",
			CodecType::Data => "data",
			CodecType::Attachment => "attachments",
			CodecType::Unknown => "streams of an unknown type",
		};
		match stream.codec_type {
			CodecType::Attachment => kind.to_owned(),
			CodecType::Data => match stream.codec_tag_string.as_deref() {
				Some(tag) if !tag.is_empty() => format!("{tag} {kind}"),
				_ => kind.to_owned(),
			},
			_ => format!(
				"{} {kind}",
				stream.codec_name.as_deref().unwrap_or("unknown-codec")
			),
		}
	}
}

impl fmt::Display for Container {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.name())
	}
}

/// A closed list: listed codecs work, every other one is refused.
fn listed(codec: &str, supported: &[&str]) -> Compatibility {
	if supported.contains(&codec) {
		Compatibility::Supported
	} else {
		Compatibility::Unsupported
	}
}

/// An open list: `supported` works, `refused` fails, the rest is uncertain.
fn one_of(codec: &str, supported: &[&str], refused: &[&str]) -> Compatibility {
	if supported.contains(&codec) {
		Compatibility::Supported
	} else if refused.contains(&codec) || codec.is_empty() {
		Compatibility::Unsupported
	} else {
		Compatibility::Uncertain
	}
}

/// Any codec ffprobe could name; one it could not cannot be copied.
fn known(codec: &str) -> Compatibility {
	if codec.is_empty() {
		Compatibility::Unsupported
	} else {
		Compatibility::Supported
	}
}

const MKV_SUBTITLES: &[&str] = &[
	"subrip",
	"ass",
	"ssa",
	"text",
	"webvtt",
	"hdmv_pgs_subtitle",
	"dvd_subtitle",
	"dvb_subtitle",
];

const MP4_VIDEO: &[&str] = &[
	"h264",
	"hevc",
	"vvc",
	"av1",
	"vp9",
	"mpeg4",
	"mpeg2video",
	"mpeg1video",
	"mjpeg",
	"png",
	"jpeg2000",
	"vc1",
	"h263",
	"ffv1",
];
const MP4_VIDEO_REFUSED: &[&str] = &["vp8", "theora", "prores", "dnxhd", "rawvideo"];
const MOV_VIDEO: &[&str] = &[
	"h264",
	"hevc",
	"av1",
	"mpeg4",
	"mpeg2video",
	"mpeg1video",
	"mjpeg",
	"png",
	"jpeg2000",
	"prores",
	"dnxhd",
	"rawvideo",
	"qtrle",
	"ffv1",
	"dvvideo",
	"cinepak",
	"svq1",
	"svq3",
];
const MOV_VIDEO_REFUSED: &[&str] = &["vp8", "vp9", "theora"];

const MP4_AUDIO: &[&str] = &[
	"aac",
	"mp3",
	"mp2",
	"mp1",
	"ac3",
	"eac3",
	"dts",
	"alac",
	"opus",
	"vorbis",
	"flac",
	"pcm_s16le",
	"pcm_s24le",
	"pcm_s32le",
	"pcm_s16be",
	"pcm_s24be",
	"pcm_s32be",
];
// "truehd in MP4 support is experimental, add '-strict -2'": a copy
// without that flag fails, and the flag is not offered.
const MP4_AUDIO_REFUSED: &[&str] = &["truehd", "mlp", "wmav1", "wmav2", "wmapro", "wmalossless"];
const MOV_AUDIO: &[&str] = &[
	"aac",
	"mp3",
	"mp2",
	"mp1",
	"ac3",
	"eac3",
	"dts",
	"alac",
	"amr_nb",
	"amr_wb",
	"adpcm_ima_qt",
];
// "opus only supported in MP4", "flac only supported in MP4", "truehd only
// supported in MP4".
const MOV_AUDIO_REFUSED: &[&str] = &[
	"opus", "flac", "truehd", "mlp", "vorbis", "wmav1", "wmav2", "wmapro",
];

const MP4_SUBTITLES: &[&str] = &["mov_text", "dvd_subtitle"];
const MOV_SUBTITLES: &[&str] = &["mov_text", "eia_608"];

#[cfg(test)]
mod tests {
	use super::*;

	fn stream(kind: CodecType, codec: &str) -> Stream {
		Stream {
			codec_type: kind,
			codec_name: (!codec.is_empty()).then(|| codec.to_owned()),
			..Stream::default()
		}
	}

	#[test]
	fn the_extension_picks_the_container() {
		let of = |name: &str| Container::from_extension(Path::new(name));
		assert_eq!(of("/out/movie.mkv"), Some(Container::Mkv));
		assert_eq!(of("movie.MKA"), Some(Container::Mkv));
		assert_eq!(of("movie.mp4"), Some(Container::Mp4));
		assert_eq!(of("song.m4a"), Some(Container::Mp4));
		assert_eq!(of("clip.mov"), Some(Container::Mov));
		assert_eq!(of("clip.webm"), Some(Container::Webm));
		assert_eq!(of("clip.avi"), None);
		assert_eq!(of("clip"), None);
		assert_eq!(Container::Webm.to_string(), "WebM");
		assert_eq!(Container::Mkv.extensions()[0], "mkv");
	}

	#[test]
	fn mkv_holds_everything_but_data_and_odd_subtitles() {
		use Compatibility::{Supported, Unsupported};
		let mkv = Container::Mkv;
		assert_eq!(mkv.holds(&stream(CodecType::Video, "h264")), Supported);
		assert_eq!(mkv.holds(&stream(CodecType::Video, "prores")), Supported);
		assert_eq!(mkv.holds(&stream(CodecType::Audio, "truehd")), Supported);
		assert_eq!(mkv.holds(&stream(CodecType::Subtitle, "subrip")), Supported);
		assert_eq!(
			mkv.holds(&stream(CodecType::Subtitle, "hdmv_pgs_subtitle")),
			Supported
		);
		assert_eq!(
			mkv.holds(&stream(CodecType::Subtitle, "mov_text")),
			Unsupported
		);
		assert_eq!(mkv.holds(&stream(CodecType::Attachment, "")), Supported);
		assert_eq!(mkv.holds(&stream(CodecType::Data, "")), Unsupported);
		assert_eq!(mkv.holds(&stream(CodecType::Video, "")), Unsupported);
	}

	#[test]
	fn mp4_refuses_text_subtitles_attachments_and_truehd() {
		use Compatibility::{Supported, Uncertain, Unsupported};
		let mp4 = Container::Mp4;
		assert_eq!(mp4.holds(&stream(CodecType::Video, "hevc")), Supported);
		assert_eq!(mp4.holds(&stream(CodecType::Video, "vp8")), Unsupported);
		assert_eq!(mp4.holds(&stream(CodecType::Video, "cfhd")), Uncertain);
		assert_eq!(mp4.holds(&stream(CodecType::Audio, "flac")), Supported);
		assert_eq!(mp4.holds(&stream(CodecType::Audio, "truehd")), Unsupported);
		assert_eq!(mp4.holds(&stream(CodecType::Audio, "tta")), Uncertain);
		assert_eq!(
			mp4.holds(&stream(CodecType::Subtitle, "subrip")),
			Unsupported
		);
		assert_eq!(mp4.holds(&stream(CodecType::Subtitle, "ass")), Unsupported);
		assert_eq!(
			mp4.holds(&stream(CodecType::Subtitle, "mov_text")),
			Supported
		);
		assert_eq!(mp4.holds(&stream(CodecType::Attachment, "")), Unsupported);
		let timecode = Stream {
			codec_tag_string: Some("tmcd".into()),
			..stream(CodecType::Data, "")
		};
		assert_eq!(mp4.holds(&timecode), Unsupported);
		assert_eq!(Container::Mov.holds(&timecode), Supported);
	}

	#[test]
	fn mov_takes_pcm_and_prores_but_not_opus() {
		use Compatibility::{Supported, Unsupported};
		let mov = Container::Mov;
		assert_eq!(mov.holds(&stream(CodecType::Audio, "pcm_s24le")), Supported);
		assert_eq!(mov.holds(&stream(CodecType::Video, "prores")), Supported);
		assert_eq!(mov.holds(&stream(CodecType::Audio, "opus")), Unsupported);
		assert_eq!(mov.holds(&stream(CodecType::Audio, "flac")), Unsupported);
		assert_eq!(
			mov.holds(&stream(CodecType::Subtitle, "subrip")),
			Unsupported
		);
	}

	#[test]
	fn webm_is_a_closed_list() {
		use Compatibility::{Supported, Unsupported};
		let webm = Container::Webm;
		assert_eq!(webm.holds(&stream(CodecType::Video, "vp9")), Supported);
		assert_eq!(webm.holds(&stream(CodecType::Video, "h264")), Unsupported);
		assert_eq!(webm.holds(&stream(CodecType::Audio, "opus")), Supported);
		assert_eq!(webm.holds(&stream(CodecType::Audio, "aac")), Unsupported);
		assert_eq!(
			webm.holds(&stream(CodecType::Subtitle, "webvtt")),
			Supported
		);
		assert_eq!(webm.holds(&stream(CodecType::Attachment, "")), Unsupported);
	}

	#[test]
	fn streams_are_described_by_codec_and_kind() {
		assert_eq!(
			Container::describe(&stream(CodecType::Subtitle, "subrip")),
			"subrip subtitles"
		);
		assert_eq!(
			Container::describe(&stream(CodecType::Attachment, "")),
			"attachments"
		);
		assert_eq!(
			Container::describe(&Stream {
				codec_tag_string: Some("tmcd".into()),
				..stream(CodecType::Data, "")
			}),
			"tmcd data"
		);
		assert_eq!(
			Container::describe(&stream(CodecType::Video, "")),
			"unknown-codec video"
		);
	}
}
