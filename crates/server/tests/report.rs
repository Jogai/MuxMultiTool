//! The stream report over HTTP: several files and a wildcard selection in
//! one call, per-file errors, sizes, and the details of one stream. Runs
//! the real ffmpeg and ffprobe from `PATH` on generated clips.

use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Duration;

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner};
use muxmultitool_server::{Config, app};
use serde_json::{Value, json};
use tower::ServiceExt;

const SERVICE: &str = "/rpc/muxmultitool.files.v1.FilesService";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with generated clips:
///
/// ```text
/// media/
///   a.mkv       H.264, AAC eng (tagged NUMBER_OF_BYTES=1), AAC fre, SRT eng
///   b.mp4       H.264, AAC eng
///   broken.mkv  not a media file
///   series/ s01e01.mkv   a copy of a.mkv
/// ```
struct Fixture {
	dir: tempfile::TempDir,
}

const CODECS: &[&str] = &[
	"-c:v",
	"libx264",
	"-preset",
	"ultrafast",
	"-pix_fmt",
	"yuv420p",
	"-c:a",
	"aac",
	"-b:a",
	"64k",
	"-metadata:s:a:0",
	"language=eng",
];

impl Fixture {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let media = dir.path().join("media");
		fs::create_dir_all(media.join("series"))?;
		let subs = dir.path().join("en.srt");
		fs::write(&subs, "1\n00:00:00,000 --> 00:00:01,000\nHello\n\n")?;
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(120));
		let mkv = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("testsrc2=size=160x120:rate=25:duration=2")
			.option("-f", "lavfi")
			.input("sine=frequency=440:sample_rate=48000:duration=2")
			.option("-f", "lavfi")
			.input("sine=frequency=660:sample_rate=48000:duration=2")
			.input(&subs)
			.args(["-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:s"])
			.args(CODECS)
			.args(["-metadata:s:a:0", "NUMBER_OF_BYTES=1"])
			.args(["-metadata:s:a:1", "language=fre", "-c:s", "srt"])
			.args(["-metadata:s:s:0", "language=eng"])
			.output(media.join("a.mkv"));
		runner.run(&mkv, &limits, None).await?;
		let mp4 = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("testsrc2=size=160x120:rate=25:duration=2")
			.option("-f", "lavfi")
			.input("sine=frequency=440:sample_rate=48000:duration=2")
			.args(CODECS)
			.output(media.join("b.mp4"));
		runner.run(&mp4, &limits, None).await?;
		fs::write(media.join("broken.mkv"), b"this is not a media file")?;
		fs::copy(media.join("a.mkv"), media.join("series/s01e01.mkv"))?;
		Ok(Self { dir })
	}

	fn media(&self) -> PathBuf {
		self.dir.path().join("media")
	}

	fn config(&self) -> Config {
		Config {
			address: IpAddr::V4(Ipv4Addr::LOCALHOST),
			port: 0,
			media_roots: vec![self.media()],
			frontend_dir: self.dir.path().join("no-frontend"),
			..Config::default()
		}
	}
}

/// Calls `method` with a JSON body and returns the status and the JSON answer.
async fn call(config: &Config, method: &str, body: Value) -> Result<(StatusCode, Value)> {
	let request = Request::post(format!("{SERVICE}/{method}"))
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_string()))?;
	let response = app(config).oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes();
	Ok((parts.status, serde_json::from_slice(&body)?))
}

/// The id of the media root and the entries under it.
async fn ids(config: &Config) -> Result<(String, Value)> {
	let (status, body) = call(config, "ListFolder", json!({})).await?;
	assert_eq!(status, StatusCode::OK, "{body}");
	let root = body["entries"][0]["id"]
		.as_str()
		.ok_or("no media root")?
		.to_owned();
	let (status, body) = call(config, "ListFolder", json!({ "folderId": root })).await?;
	assert_eq!(status, StatusCode::OK, "{body}");
	Ok((root, body["entries"].clone()))
}

fn id_of<'a>(entries: &'a Value, name: &str) -> Option<&'a str> {
	entries.as_array()?.iter().find(|e| e["name"] == name)?["id"].as_str()
}

/// `(index, type, codec, language, size, method)` of each stream, as the
/// JSON encoding prints them: a zero index and an unspecified method are
/// left out.
fn rows(file: &Value) -> Vec<(u64, &str, &str, &str, u64, &str)> {
	file["streams"]
		.as_array()
		.into_iter()
		.flatten()
		.map(|s| {
			(
				s["index"].as_u64().unwrap_or(0),
				s["type"].as_str().unwrap_or(""),
				s["codec"].as_str().unwrap_or(""),
				s["language"].as_str().unwrap_or(""),
				s["sizeBytes"]
					.as_str()
					.map_or(0, |b| b.parse().unwrap_or(0)),
				s["sizeMethod"].as_str().unwrap_or(""),
			)
		})
		.collect()
}

/// The full report of the fixture: four ids (one twice, one garbage, one a
/// folder) and a wildcard selection over the subfolders.
async fn full_report(fixture: &Fixture) -> Result<Value> {
	let config = fixture.config();
	let (root, entries) = ids(&config).await?;
	let b = id_of(&entries, "b.mp4").ok_or("b.mp4")?;
	let broken = id_of(&entries, "broken.mkv").ok_or("broken.mkv")?;
	let series = id_of(&entries, "series").ok_or("series")?;
	let (status, body) = call(
		&config,
		"GetStreamReport",
		json!({
			"fileIds": [b, broken, "garbage", series, b],
			"selections": [{ "folderId": root, "patterns": ["*.MKV"], "recursive": true }],
		}),
	)
	.await?;
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(body["files"][0]["fileId"], b);
	assert_eq!(body["files"][1]["fileId"], broken);
	Ok(body)
}

#[tokio::test]
async fn reports_ids_and_wildcards_in_one_call() {
	let fixture = Fixture::new().await.unwrap();
	let body = full_report(&fixture).await.unwrap();
	let files = body["files"].as_array().unwrap();

	// The ids in order, once each, then the selection without the files already listed
	let names: Vec<_> = files
		.iter()
		.map(|f| f["entry"]["name"].as_str().unwrap_or("-"))
		.collect();
	assert_eq!(
		names,
		["b.mp4", "broken.mkv", "-", "series", "a.mkv", "s01e01.mkv"]
	);

	// A file ffprobe cannot read: its own error, the call went on
	let bad = &files[1];
	assert_eq!(bad["entry"]["name"], "broken.mkv");
	assert_eq!(
		bad["error"]["message"],
		"the input is not a media file ffmpeg can read"
	);
	let output = bad["error"]["ffprobeOutput"].as_str().unwrap();
	assert!(output.contains("broken.mkv"), "{output}");
	assert!(
		!output.contains(fixture.media().to_str().unwrap()),
		"a server path leaked: {output}"
	);
	assert!(bad.get("streams").is_none(), "{bad}");

	// An id that is not the server's, and a folder
	assert_eq!(files[2]["fileId"], "garbage");
	assert!(files[2].get("entry").is_none(), "{}", files[2]);
	assert!(
		files[2]["error"]["message"]
			.as_str()
			.unwrap()
			.starts_with("invalid id"),
		"{}",
		files[2]
	);
	assert_eq!(files[3]["error"]["message"], "a folder, not a file");
	assert_eq!(files[5]["entry"]["path"], "series/s01e01.mkv");
	assert_eq!(rows(&files[5]), rows(&files[4]));
}

#[tokio::test]
async fn measured_streams_have_exact_sizes() {
	let fixture = Fixture::new().await.unwrap();
	let body = full_report(&fixture).await.unwrap();
	let files = body["files"].as_array().unwrap();

	// An MP4 with two streams
	let mp4 = &files[0];
	assert_eq!(mp4["container"]["format"], "mov,mp4,m4a,3gp,3g2,mj2");
	assert_eq!(
		mp4["container"]["sizeBytes"].as_str().unwrap(),
		fs::metadata(fixture.media().join("b.mp4"))
			.unwrap()
			.len()
			.to_string()
	);
	assert!(mp4["container"]["sizeGb"].as_f64().unwrap() > 0.0);
	assert!(mp4["container"]["durationSeconds"].as_f64().unwrap() > 1.9);
	assert!(mp4.get("error").is_none(), "{mp4}");
	let streams = rows(mp4);
	assert_eq!(streams.len(), 2);
	assert_eq!(
		(streams[0].0, streams[0].1, streams[0].2, streams[0].5),
		(0, "STREAM_TYPE_VIDEO", "h264", "SIZE_METHOD_PACKETS")
	);
	assert_eq!(
		(
			streams[1].0,
			streams[1].1,
			streams[1].2,
			streams[1].3,
			streams[1].5
		),
		(1, "STREAM_TYPE_AUDIO", "aac", "eng", "SIZE_METHOD_PACKETS")
	);
	assert!(
		streams[0].4 > streams[1].4 && streams[1].4 > 0,
		"{streams:?}"
	);
	let video = &mp4["streams"][0];
	assert_eq!(video["width"], 160);
	assert_eq!(video["frameRate"], 25.0);
	assert!(video["sizeGb"].as_f64().unwrap() > 0.0);

	// The MKV from the wildcard: four streams with their languages
	let mkv = &files[4];
	assert_eq!(mkv["container"]["format"], "matroska,webm");
	let streams = rows(mkv);
	let summary: Vec<_> = streams
		.iter()
		.map(|(i, t, c, l, _, m)| (*i, *t, *c, *l, *m))
		.collect();
	assert_eq!(
		summary,
		[
			(0, "STREAM_TYPE_VIDEO", "h264", "", "SIZE_METHOD_PACKETS"),
			(1, "STREAM_TYPE_AUDIO", "aac", "eng", "SIZE_METHOD_PACKETS"),
			(2, "STREAM_TYPE_AUDIO", "aac", "fre", "SIZE_METHOD_PACKETS"),
			(
				3,
				"STREAM_TYPE_SUBTITLE",
				"subrip",
				"eng",
				"SIZE_METHOD_PACKETS"
			),
		]
	);
	// Measured, so the hand-written tag (1 byte) was not believed
	assert!(streams[1].4 > 1000, "{streams:?}");
	assert_eq!(mkv["streams"][1]["flags"], json!(["default"]));
	assert_eq!(mkv["streams"][1]["channels"], 1);
	assert_eq!(mkv["streams"][1]["sampleRate"], 48000);
}

#[tokio::test]
async fn quick_reports_use_tags_and_estimates() {
	let fixture = Fixture::new().await.unwrap();
	let config = fixture.config();
	let (_, entries) = ids(&config).await.unwrap();

	let (status, body) = call(
		&config,
		"GetStreamReport",
		json!({
			"fileIds": [id_of(&entries, "a.mkv").unwrap(), id_of(&entries, "b.mp4").unwrap()],
			"quick": true,
		}),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	let mkv = rows(&body["files"][0]);
	// Only the tagged stream has a size: Matroska gives no bit rate to estimate from
	assert_eq!((mkv[0].4, mkv[0].5), (0, ""));
	assert_eq!((mkv[1].4, mkv[1].5), (1, "SIZE_METHOD_TAGS"));
	assert_eq!((mkv[2].4, mkv[2].5), (0, ""));
	let mp4 = rows(&body["files"][1]);
	assert!(
		mp4.iter()
			.all(|row| row.5 == "SIZE_METHOD_ESTIMATED" && row.4 > 0),
		"{mp4:?}"
	);

	// Nothing selected is an empty report, not an error
	let (status, body) = call(&config, "GetStreamReport", json!({})).await.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(body, json!({}));
}

#[tokio::test]
async fn bad_selections_fail_the_call() {
	let fixture = Fixture::new().await.unwrap();
	let config = fixture.config();
	let (root, _) = ids(&config).await.unwrap();

	let (status, body) = call(
		&config,
		"GetStreamReport",
		json!({ "selections": [{ "folderId": root, "patterns": ["[oops"] }] }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	assert_eq!(body["code"], "invalid_argument");

	let (status, body) = call(
		&config,
		"GetStreamReport",
		json!({ "selections": [{ "folderId": "garbage", "patterns": ["*"] }] }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	assert_eq!(body["code"], "invalid_argument");
}

#[tokio::test]
async fn details_of_one_stream() {
	let fixture = Fixture::new().await.unwrap();
	let config = fixture.config();
	let (_, entries) = ids(&config).await.unwrap();
	let a = id_of(&entries, "a.mkv").unwrap();

	let (status, body) = call(
		&config,
		"GetStreamDetails",
		json!({ "fileId": a, "streamIndex": 2 }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{body}");
	let fields = body["fields"].as_array().unwrap();
	assert_eq!(fields[0], json!({ "key": "index", "value": "2" }));
	assert!(
		fields.contains(&json!({ "key": "codec_name", "value": "aac" })),
		"{body}"
	);
	assert!(
		fields.contains(&json!({ "key": "tags.language", "value": "fre" })),
		"{body}"
	);
	assert!(
		fields.contains(&json!({ "key": "disposition.default", "value": "0" })),
		"{body}"
	);
	let raw: Value = serde_json::from_str(body["json"].as_str().unwrap()).unwrap();
	assert_eq!(raw["codec_name"], "aac");
	assert_eq!(raw["tags"]["language"], "fre");

	let (status, body) = call(
		&config,
		"GetStreamDetails",
		json!({ "fileId": a, "streamIndex": 9 }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
	assert_eq!(body["code"], "not_found");

	let (status, body) = call(
		&config,
		"GetStreamDetails",
		json!({ "fileId": id_of(&entries, "broken.mkv").unwrap() }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	assert_eq!(body["code"], "failed_precondition");
	assert_eq!(
		body["message"],
		"the input is not a media file ffmpeg can read"
	);

	let (status, body) = call(&config, "GetStreamDetails", json!({ "fileId": "garbage" }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	assert_eq!(body["code"], "invalid_argument");

	// A missing file is not found, as elsewhere in the service
	fs::remove_file(fixture.media().join("a.mkv")).unwrap();
	let (status, body) = call(&config, "GetStreamDetails", json!({ "fileId": a }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
	assert_eq!(body["code"], "not_found");
}
