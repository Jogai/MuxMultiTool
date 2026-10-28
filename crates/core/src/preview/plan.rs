//! What to do with a source file: which streams, copy or encode, and the
//! playlist that follows from that.

use crate::ffmpeg::probe::{CodecType, ProbeResult, Stream};

use super::playlist::Playlist;

/// Widest preview picture. Wider sources are scaled down for the transcode;
/// the preview is a proxy, exports come from the original.
pub const MAX_WIDTH: u32 = 1920;

/// How the preview is produced from the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
	/// Video and audio are already H.264 and AAC: both are copied.
	Remux,
	/// The video is H.264 and copied; only the audio is transcoded to AAC.
	TranscodeAudio,
	/// The video is transcoded to H.264 with x264, the audio to AAC when needed.
	Transcode,
}

/// The streams of a source and what happens to each of them.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
	/// Index of the video stream (`-map 0:{index}`), or `None` for audio-only files.
	pub video: Option<u32>,
	/// Index of the audio stream, or `None` when the file has no audio.
	pub audio: Option<u32>,
	/// Whether the video stream is copied rather than encoded.
	pub video_copy: bool,
	/// Whether the audio stream is copied rather than encoded.
	pub audio_copy: bool,
	/// Whether the transcode scales the picture down to [`MAX_WIDTH`].
	pub downscale: bool,
	/// The segments.
	pub playlist: Playlist,
}

impl Plan {
	/// The mode this plan amounts to.
	#[must_use]
	pub fn mode(&self) -> Mode {
		match (self.video_copy, self.audio_copy) {
			(true, true) => Mode::Remux,
			(true, false) => Mode::TranscodeAudio,
			(false, _) => Mode::Transcode,
		}
	}
}

/// What was picked from a probe before the playlist is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Streams {
	/// See [`Plan::video`].
	pub video: Option<u32>,
	/// See [`Plan::audio`].
	pub audio: Option<u32>,
	/// Whether the video can be copied: H.264, 8-bit 4:2:0, which every browser decodes.
	pub video_copy: bool,
	/// Whether the audio can be copied: AAC.
	pub audio_copy: bool,
	/// Whether the picture is wider than [`MAX_WIDTH`].
	pub downscale: bool,
}

impl Streams {
	/// Picks the streams of `probe`: the first real video stream (cover art
	/// is skipped) and the default audio stream, or the first one.
	/// `None` when the file has neither video nor audio.
	#[must_use]
	pub fn pick(probe: &ProbeResult) -> Option<Self> {
		let video = probe
			.streams_of(CodecType::Video)
			.find(|s| !s.disposition.attached_pic);
		let audio = probe
			.streams_of(CodecType::Audio)
			.find(|s| s.disposition.default)
			.or_else(|| probe.streams_of(CodecType::Audio).next());
		if video.is_none() && audio.is_none() {
			return None;
		}
		Some(Self {
			video: video.map(|s| s.index),
			audio: audio.map(|s| s.index),
			video_copy: video.is_none_or(video_is_playable),
			audio_copy: audio.is_none_or(audio_is_playable),
			downscale: video.is_some_and(|s| s.width.is_some_and(|w| w > MAX_WIDTH)),
		})
	}
}

/// H.264 with 8-bit 4:2:0 samples. `Hi10P` and 4:2:2/4:4:4 are H.264 too, but
/// browsers do not decode them.
fn video_is_playable(stream: &Stream) -> bool {
	stream.codec_name.as_deref() == Some("h264")
		&& matches!(stream.pix_fmt.as_deref(), Some("yuv420p" | "yuvj420p"))
}

fn audio_is_playable(stream: &Stream) -> bool {
	stream.codec_name.as_deref() == Some("aac")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe::parse;

	fn probe(streams: &str) -> ProbeResult {
		parse(&format!(
			r#"{{"streams":[{streams}],"format":{{"duration":"10"}}}}"#
		))
		.unwrap()
	}

	#[test]
	fn h264_aac_is_a_remux() {
		let streams = Streams::pick(&probe(
			r#"{"index":0,"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p","width":1920},
			{"index":1,"codec_type":"audio","codec_name":"aac"}"#,
		))
		.unwrap();
		assert_eq!(
			streams,
			Streams {
				video: Some(0),
				audio: Some(1),
				video_copy: true,
				audio_copy: true,
				downscale: false,
			}
		);
	}

	#[test]
	fn hevc_dts_is_a_transcode_scaled_to_1080p() {
		let streams = Streams::pick(&probe(
			r#"{"index":0,"codec_type":"video","codec_name":"hevc","pix_fmt":"yuv420p10le","width":3840},
			{"index":1,"codec_type":"audio","codec_name":"dts"}"#,
		))
		.unwrap();
		assert!(!streams.video_copy);
		assert!(!streams.audio_copy);
		assert!(streams.downscale);
	}

	#[test]
	fn h264_with_dts_copies_the_video_only() {
		let streams = Streams::pick(&probe(
			r#"{"index":0,"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p"},
			{"index":1,"codec_type":"audio","codec_name":"dts"}"#,
		))
		.unwrap();
		assert!(streams.video_copy);
		assert!(!streams.audio_copy);
	}

	#[test]
	fn ten_bit_h264_is_transcoded() {
		let streams = Streams::pick(&probe(
			r#"{"index":0,"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p10le"}"#,
		))
		.unwrap();
		assert!(!streams.video_copy);
		assert!(streams.audio_copy, "no audio: nothing to transcode");
		assert_eq!(streams.audio, None);
	}

	#[test]
	fn cover_art_is_not_the_video_and_default_audio_wins() {
		let streams = Streams::pick(&probe(
			r#"{"index":0,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}},
			{"index":1,"codec_type":"audio","codec_name":"ac3","tags":{"language":"fra"}},
			{"index":2,"codec_type":"audio","codec_name":"aac","disposition":{"default":1}},
			{"index":3,"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p"}"#,
		))
		.unwrap();
		assert_eq!((streams.video, streams.audio), (Some(3), Some(2)));
		assert!(streams.video_copy && streams.audio_copy);
	}

	#[test]
	fn no_media_streams_is_none() {
		assert!(Streams::pick(&probe(r#"{"index":0,"codec_type":"subtitle"}"#)).is_none());
	}

	#[test]
	fn mode_follows_the_copy_flags() {
		let plan = |video_copy, audio_copy| Plan {
			video: Some(0),
			audio: Some(1),
			video_copy,
			audio_copy,
			downscale: false,
			playlist: Playlist::grid(1.0, 1.0),
		};
		assert_eq!(plan(true, true).mode(), Mode::Remux);
		assert_eq!(plan(true, false).mode(), Mode::TranscodeAudio);
		assert_eq!(plan(false, true).mode(), Mode::Transcode);
		assert_eq!(plan(false, false).mode(), Mode::Transcode);
	}
}
