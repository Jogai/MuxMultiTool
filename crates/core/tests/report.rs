//! The per-stream report, first from recorded ffprobe output (an MP4 and
//! an MKV with several audio and subtitle languages, under
//! `fixtures/ffprobe/`), then against the real ffprobe from `PATH` on clips
//! generated with ffmpeg's test sources.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::ffmpeg::probe::{self, CodecType, PacketSizes};
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner};
use muxmultitool_core::report::{self, Report, ReportOptions, SizeMethod};

const MKV_JSON: &str = include_str!("fixtures/ffprobe/mkv.json");
const MP4_JSON: &str = include_str!("fixtures/ffprobe/mp4.json");
const MKV_PACKETS: &str = include_str!("fixtures/ffprobe/mkv-packets.csv");
const MP4_PACKETS: &str = include_str!("fixtures/ffprobe/mp4-packets.csv");

fn packets(csv: &str) -> BTreeMap<u32, u64> {
	let mut sizes = PacketSizes::default();
	sizes.push_lines(csv);
	sizes.into_bytes()
}

/// `(index, type, codec, language, size, method)`: the columns of the report.
type Row<'a> = (
	u32,
	CodecType,
	&'a str,
	Option<&'a str>,
	Option<u64>,
	SizeMethod,
);

fn rows(report: &Report) -> Vec<Row<'_>> {
	report
		.streams
		.iter()
		.map(|s| {
			(
				s.index,
				s.kind,
				s.codec.as_str(),
				s.language.as_deref(),
				s.size,
				s.size_method,
			)
		})
		.collect()
}

#[test]
fn mkv_report_from_recorded_output() {
	let probe = probe::parse(MKV_JSON).unwrap();
	assert!(report::needs_measuring(&probe));

	// Quick: the one tagged stream has a size, the attachment its data size,
	// nothing else (Matroska has no per-stream bit rate to estimate from).
	let quick = report::build(&probe, None);
	assert_eq!(quick.container.format, "matroska,webm");
	assert_eq!(quick.container.title.as_deref(), Some("Fixture MKV"));
	assert_eq!(quick.container.size, Some(120_617));
	assert_eq!(quick.container.duration, Some(2.021));
	assert_eq!(
		rows(&quick),
		[
			(0, CodecType::Video, "h264", None, None, SizeMethod::Unknown),
			(
				1,
				CodecType::Audio,
				"aac",
				Some("eng"),
				Some(16_000),
				SizeMethod::Tags
			),
			(
				2,
				CodecType::Audio,
				"flac",
				Some("nld"),
				None,
				SizeMethod::Unknown
			),
			(
				3,
				CodecType::Subtitle,
				"subrip",
				Some("eng"),
				None,
				SizeMethod::Unknown
			),
			(
				4,
				CodecType::Subtitle,
				"ass",
				Some("fre"),
				None,
				SizeMethod::Unknown
			),
			(
				5,
				CodecType::Attachment,
				"ttf",
				None,
				Some(8),
				SizeMethod::Extradata
			),
		]
	);
}

#[test]
fn mkv_report_measured_from_recorded_packets() {
	let probe = probe::parse(MKV_JSON).unwrap();
	// Every stream from the packet pass, the tag included (it was written by
	// hand and is wrong); the attachment keeps its data size.
	let measured = report::build(&probe, Some(&packets(MKV_PACKETS)));
	assert_eq!(
		rows(&measured),
		[
			(
				0,
				CodecType::Video,
				"h264",
				None,
				Some(66_436),
				SizeMethod::Packets
			),
			(
				1,
				CodecType::Audio,
				"aac",
				Some("eng"),
				Some(16_058),
				SizeMethod::Packets
			),
			(
				2,
				CodecType::Audio,
				"flac",
				Some("nld"),
				Some(34_297),
				SizeMethod::Packets
			),
			(
				3,
				CodecType::Subtitle,
				"subrip",
				Some("eng"),
				Some(10),
				SizeMethod::Packets
			),
			(
				4,
				CodecType::Subtitle,
				"ass",
				Some("fre"),
				Some(27),
				SizeMethod::Packets
			),
			(
				5,
				CodecType::Attachment,
				"ttf",
				None,
				Some(8),
				SizeMethod::Extradata
			),
		]
	);
	let total: u64 = measured.streams.iter().filter_map(|s| s.size).sum();
	assert!(total < 120_617, "streams cannot exceed the file: {total}");

	let dutch = &measured.streams[2];
	assert_eq!(dutch.title.as_deref(), Some("Dutch"));
	assert_eq!(dutch.channels, Some(1));
	assert_eq!(dutch.sample_rate, Some(48_000));
	// The container duration stands in for a stream without one
	assert_eq!(dutch.duration, Some(2.021));
	assert_eq!(measured.streams[1].flags, ["default"]);
	assert_eq!(measured.streams[4].flags, ["forced"]);
	let video = &measured.streams[0];
	assert_eq!((video.width, video.height), (Some(160), Some(120)));
	assert_eq!(video.frame_rate, Some(25.0));
	assert_eq!(video.pix_fmt.as_deref(), Some("yuv420p"));
	assert_eq!(video.profile.as_deref(), Some("Constrained Baseline"));
}

#[test]
fn mp4_report_from_recorded_output() {
	let probe = probe::parse(MP4_JSON).unwrap();
	assert!(report::needs_measuring(&probe));

	// Quick: MP4 has a bit rate per stream, so every stream is estimated
	let quick = report::build(&probe, None);
	assert_eq!(quick.container.format, "mov,mp4,m4a,3gp,3g2,mj2");
	assert_eq!(quick.container.title.as_deref(), Some("Fixture MP4"));
	assert_eq!(
		rows(&quick),
		[
			(
				0,
				CodecType::Video,
				"h264",
				Some("und"),
				Some(66_436),
				SizeMethod::Estimated
			),
			(
				1,
				CodecType::Audio,
				"aac",
				Some("eng"),
				Some(15_847),
				SizeMethod::Estimated
			),
			(
				2,
				CodecType::Audio,
				"ac3",
				Some("fre"),
				Some(24_000),
				SizeMethod::Estimated
			),
			(
				3,
				CodecType::Audio,
				"aac",
				Some("deu"),
				Some(16_855),
				SizeMethod::Estimated
			),
			(
				4,
				CodecType::Subtitle,
				"mov_text",
				Some("eng"),
				Some(16),
				SizeMethod::Estimated
			),
			(
				5,
				CodecType::Subtitle,
				"mov_text",
				Some("fre"),
				Some(11),
				SizeMethod::Estimated
			),
		]
	);
	assert_eq!(quick.streams[2].title.as_deref(), Some("Commentary"));
}

#[test]
fn mp4_report_measured_from_recorded_packets() {
	let probe = probe::parse(MP4_JSON).unwrap();
	let quick = report::build(&probe, None);
	let measured = report::build(&probe, Some(&packets(MP4_PACKETS)));
	assert_eq!(
		rows(&measured),
		[
			(
				0,
				CodecType::Video,
				"h264",
				Some("und"),
				Some(66_436),
				SizeMethod::Packets
			),
			(
				1,
				CodecType::Audio,
				"aac",
				Some("eng"),
				Some(16_058),
				SizeMethod::Packets
			),
			(
				2,
				CodecType::Audio,
				"ac3",
				Some("fre"),
				Some(24_192),
				SizeMethod::Packets
			),
			(
				3,
				CodecType::Audio,
				"aac",
				Some("deu"),
				Some(17_220),
				SizeMethod::Packets
			),
			(
				4,
				CodecType::Subtitle,
				"mov_text",
				Some("eng"),
				Some(14),
				SizeMethod::Packets
			),
			(
				5,
				CodecType::Subtitle,
				"mov_text",
				Some("fre"),
				Some(9),
				SizeMethod::Packets
			),
		]
	);
	// The estimates are near the measured sizes: within 5 % for the audio
	for (estimate, exact) in quick.streams.iter().zip(&measured.streams).take(4) {
		let (e, x) = (estimate.size.unwrap(), exact.size.unwrap());
		assert!(e.abs_diff(x) * 20 < x, "#{}: {e} vs {x}", exact.index);
	}
}

#[test]
fn details_from_recorded_output() {
	let details = report::stream_details(MKV_JSON, 2).unwrap().unwrap();
	let field = |key: &str| {
		details
			.fields
			.iter()
			.find(|(k, _)| k == key)
			.map(|(_, v)| v.as_str())
	};
	assert_eq!(details.fields[0], ("index".to_owned(), "2".to_owned()));
	assert_eq!(details.fields[1].0, "codec_name");
	assert_eq!(field("codec_name"), Some("flac"));
	assert_eq!(field("sample_fmt"), Some("s16"));
	assert_eq!(field("tags.language"), Some("nld"));
	assert_eq!(field("tags.title"), Some("Dutch"));
	assert_eq!(field("disposition.default"), Some("0"));
	assert_eq!(field("extradata_size"), Some("34"));
	let json: serde_json::Value = serde_json::from_str(&details.json).unwrap();
	assert_eq!(json["codec_name"], "flac");
	assert_eq!(json["tags"]["language"], "nld");

	// The attachment, and an index that is not there
	let font = report::stream_details(MKV_JSON, 5).unwrap().unwrap();
	assert_eq!(
		font.fields
			.iter()
			.find(|(k, _)| k == "tags.filename")
			.map(|(_, v)| v.as_str()),
		Some("font.ttf")
	);
	assert_eq!(report::stream_details(MP4_JSON, 6).unwrap(), None);
}

/// A two-second clip in `dir` with the streams the report must tell apart:
/// H.264 video, English and French audio, an English subtitle track; `extra`
/// goes before the output.
async fn make_clip(
	runner: &Runner,
	dir: &Path,
	name: &str,
	extra: &[&str],
) -> Result<PathBuf, Box<dyn std::error::Error>> {
	let subs = dir.join("en.srt");
	std::fs::write(&subs, "1\n00:00:00,000 --> 00:00:01,000\nHello\n\n")?;
	let clip = dir.join(name);
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=size=160x120:rate=25:duration=2")
		.option("-f", "lavfi")
		.input("sine=frequency=440:sample_rate=48000:duration=2")
		.option("-f", "lavfi")
		.input("sine=frequency=660:sample_rate=48000:duration=2")
		.input(&subs)
		.args(["-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:s"])
		.args([
			"-c:v",
			"libx264",
			"-preset",
			"ultrafast",
			"-pix_fmt",
			"yuv420p",
		])
		.args(["-c:a", "aac", "-b:a", "64k"])
		.args([
			"-metadata:s:a:0",
			"language=eng",
			"-metadata:s:a:1",
			"language=fre",
		])
		.args(["-metadata:s:s:0", "language=eng"])
		.args(extra)
		.output(&clip);
	runner
		.run(&cmd, &Limits::timeout(Duration::from_secs(120)), None)
		.await?;
	Ok(clip)
}

#[tokio::test]
async fn real_files_are_measured_and_estimated() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let mkv = make_clip(
		&runner,
		dir.path(),
		"clip.mkv",
		&["-c:s", "srt", "-metadata:s:a:0", "NUMBER_OF_BYTES=1"],
	)
	.await
	.unwrap();
	let mp4 = make_clip(&runner, dir.path(), "clip.mp4", &["-c:s", "mov_text"])
		.await
		.unwrap();
	let file_size = std::fs::metadata(&mkv).unwrap().len();

	let measured = report::report(&runner, &mkv, &ReportOptions::default())
		.await
		.unwrap();
	let kinds: Vec<_> = measured
		.streams
		.iter()
		.map(|s| (s.kind, s.language.as_deref()))
		.collect();
	assert_eq!(
		kinds,
		[
			(CodecType::Video, None),
			(CodecType::Audio, Some("eng")),
			(CodecType::Audio, Some("fre")),
			(CodecType::Subtitle, Some("eng")),
		]
	);
	assert!(
		measured
			.streams
			.iter()
			.all(|s| s.size_method == SizeMethod::Packets),
		"{measured:?}"
	);
	let total: u64 = measured.streams.iter().filter_map(|s| s.size).sum();
	assert!(
		total > file_size / 2 && total < file_size,
		"streams {total} of file {file_size}"
	);
	assert!(measured.streams[0].size > measured.streams[1].size);
	assert!(
		measured.streams[1].size.unwrap() > 1,
		"the tag was overridden"
	);

	let quick = report::report(
		&runner,
		&mkv,
		&ReportOptions {
			quick: true,
			..ReportOptions::default()
		},
	)
	.await
	.unwrap();
	assert_eq!(quick.streams[1].size, Some(1));
	assert_eq!(quick.streams[1].size_method, SizeMethod::Tags);
	assert_eq!(quick.streams[0].size_method, SizeMethod::Unknown);

	let quick = report::report(
		&runner,
		&mp4,
		&ReportOptions {
			quick: true,
			..ReportOptions::default()
		},
	)
	.await
	.unwrap();
	assert_eq!(quick.container.format, "mov,mp4,m4a,3gp,3g2,mj2");
	assert!(
		quick
			.streams
			.iter()
			.all(|s| s.size_method == SizeMethod::Estimated),
		"{quick:?}"
	);

	let details = report::details(&runner, &mp4, 2, &ReportOptions::default())
		.await
		.unwrap()
		.unwrap();
	assert!(
		details
			.fields
			.contains(&("tags.language".to_owned(), "fre".to_owned())),
		"{:?}",
		details.fields
	);
	assert_eq!(
		report::details(&runner, &mp4, 9, &ReportOptions::default())
			.await
			.unwrap(),
		None
	);
}

#[tokio::test]
async fn unreadable_files_get_a_plain_error_without_the_path() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let broken = dir.path().join("broken.mkv");
	std::fs::write(&broken, b"this is not a media file").unwrap();

	let error = report::report(&runner, &broken, &ReportOptions::default())
		.await
		.unwrap_err();
	assert_eq!(
		error.message,
		"the input is not a media file ffmpeg can read"
	);
	assert!(error.ffprobe_output.contains("broken.mkv"), "{error:?}");
	assert!(
		!error.ffprobe_output.contains(dir.path().to_str().unwrap()),
		"{error:?}"
	);

	let missing = dir.path().join("missing.mkv");
	let error = report::report(&runner, &missing, &ReportOptions::default())
		.await
		.unwrap_err();
	assert_eq!(error.message, "the input file was not found");

	let error = report::details(&runner, &broken, 0, &ReportOptions::default())
		.await
		.unwrap_err();
	assert_eq!(
		error.message,
		"the input is not a media file ffmpeg can read"
	);
}
