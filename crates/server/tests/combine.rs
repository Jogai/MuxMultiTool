//! Combining sources end to end over the Connect router, as the dev
//! frontend uses it (JSON): the preview with its sources, tracks, warnings
//! and command, the job with its output probed for the chosen tracks and
//! their metadata, a submit refused for a container that cannot hold a
//! track, a source given as a file id, and the request validation. Needs
//! ffmpeg and ffprobe on `PATH`.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, ProbeResult, Runner};
use muxmultitool_server::{Config, app};
use serde_json::{Value, json};
use tower::ServiceExt;

const SUBMIT: &str = "/rpc/muxmultitool.jobs.v1.JobsService/SubmitJob";
const GET: &str = "/rpc/muxmultitool.jobs.v1.JobsService/GetJob";
const PREVIEW: &str = "/rpc/muxmultitool.combine.v1.CombineService/PreviewCombine";
const LIST_FOLDER: &str = "/rpc/muxmultitool.files.v1.FilesService/ListFolder";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with `a.mkv` (two seconds, H.264 and English AAC),
/// `b.mkv` (four seconds, H.264, French AAC titled "French", an English
/// SubRip track) and `subs.srt` in it, plus an `out` folder.
struct MediaRoot {
	dir: tempfile::TempDir,
	a: PathBuf,
	b: PathBuf,
	srt: PathBuf,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let runner = Runner::default();
		let limits = Limits::timeout(Duration::from_secs(60));
		let srt = dir.path().join("subs.srt");
		std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,000\nhello\n\n")?;
		let a = dir.path().join("a.mkv");
		let cmd = clip(2, None)
			.args(["-map", "0", "-map", "1"])
			.option("-metadata:s:a:0", "language=eng")
			.output(&a);
		runner.run(&cmd, &limits, None).await?;
		let b = dir.path().join("b.mkv");
		let cmd = clip(4, Some(&srt))
			.args(["-map", "0", "-map", "1", "-map", "2"])
			.option("-metadata:s:a:0", "language=fre")
			.option("-metadata:s:a:0", "title=French")
			.option("-metadata:s:s:0", "language=eng")
			.output(&b);
		runner.run(&cmd, &limits, None).await?;
		std::fs::create_dir(dir.path().join("out"))?;
		Ok(Self { dir, a, b, srt })
	}

	fn path(&self) -> &Path {
		self.dir.path()
	}

	fn config(&self) -> Config {
		Config {
			address: IpAddr::V4(Ipv4Addr::LOCALHOST),
			port: 0,
			media_roots: vec![self.path().to_path_buf()],
			frontend_dir: self.path().join("no-frontend"),
			..Config::default()
		}
	}

	async fn probe(&self, name: &str) -> Result<ProbeResult> {
		Ok(Runner::default()
			.probe(
				&self.path().join(name),
				&Limits::timeout(Duration::from_secs(30)),
			)
			.await?)
	}
}

/// `secs` seconds of test pattern and tone (inputs 0 and 1), plus `subs`
/// as input 2 when given, encoded as H.264 + AAC (+ SubRip); the mapping
/// and the output are still to add.
fn clip(secs: u32, subs: Option<&Path>) -> FfmpegCommand {
	let mut cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input(format!("testsrc2=duration={secs}:size=64x64:rate=10"))
		.option("-f", "lavfi")
		.input(format!("sine=frequency=440:duration={secs}"));
	if let Some(subs) = subs {
		cmd = cmd.input(subs).option("-c:s", "srt");
	}
	cmd.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-pix_fmt", "yuv420p")
		.option("-c:a", "aac")
}

/// `(codec_type, codec_name)` of every stream, in container order.
fn streams(probe: &ProbeResult) -> Vec<(CodecType, String)> {
	probe
		.streams
		.iter()
		.map(|s| (s.codec_type, s.codec_name.clone().unwrap_or_default()))
		.collect()
}

/// Sends a JSON request to the in-process router and returns the status
/// and the decoded body.
async fn post_json(router: &axum::Router, path: &str, body: Value) -> Result<(StatusCode, Value)> {
	let request = Request::post(path)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_string()))?;
	let response = router.clone().oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes();
	Ok((parts.status, serde_json::from_slice(&body)?))
}

/// Submits `operation` as a `combine` job and waits for it to end.
async fn run_job(router: &axum::Router, operation: Value) -> Result<Value> {
	let (status, json) = post_json(router, SUBMIT, json!({ "combine": operation })).await?;
	if status != StatusCode::OK {
		return Err(format!("submit refused: {json}").into());
	}
	let id = json["job"]["id"].as_str().unwrap_or_default().to_owned();
	let job = tokio::time::timeout(Duration::from_secs(60), async {
		loop {
			let (_, json) = post_json(router, GET, json!({ "jobId": id })).await?;
			let state = json["job"]["state"].as_str().unwrap_or_default();
			if state != "JOB_STATE_QUEUED" && state != "JOB_STATE_RUNNING" {
				return Ok::<_, Box<dyn std::error::Error>>(json["job"].clone());
			}
			tokio::time::sleep(Duration::from_millis(50)).await;
		}
	})
	.await??;
	Ok(job)
}

/// A proto3 JSON integer, which is a string for 64-bit fields.
fn int(value: &Value) -> i64 {
	value
		.as_i64()
		.or_else(|| value.as_str().and_then(|s| s.parse().ok()))
		.unwrap_or_default()
}

fn origins(preview: &Value) -> Vec<&str> {
	preview["tracks"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|t| t["origin"].as_str())
		.collect()
}

/// The BD rip + Blu-ray case: A's video, B's audio shifted by half a
/// second and retitled, C's subtitles with language, title and the forced
/// flag set.
fn three_sources(root: &MediaRoot) -> Value {
	json!({
		"sources": [
			{ "input": root.a },
			{ "input": root.b, "offsetSeconds": 0.5 },
			{ "input": root.srt },
		],
		"tracks": [
			{ "source": 0, "streamIndex": 0 },
			{ "source": 1, "streamIndex": 1, "title": "Atmos", "default": true },
			{ "source": 2, "streamIndex": 0, "language": "eng", "title": "English", "forced": true },
		],
		"output": "movie.mkv",
	})
}

#[tokio::test]
async fn the_preview_names_every_source_and_track_and_warns_about_durations() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let operation = three_sources(&root);
	let (status, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{preview}");
	assert_eq!(preview["container"], "MKV");
	assert_eq!(preview["outputName"], "movie.mkv");
	assert_eq!(preview["canRun"], true);
	assert!(
		preview["issues"].as_array().is_none_or(Vec::is_empty),
		"{preview}"
	);

	// The sources, labelled and probed.
	let sources = preview["sources"].as_array().unwrap();
	assert_eq!(sources.len(), 3);
	assert_eq!(sources[0]["label"], "A");
	assert_eq!(sources[0]["name"], "a.mkv");
	assert_eq!(sources[0]["used"], true);
	assert_eq!(sources[0]["streams"].as_array().unwrap().len(), 2);
	assert_eq!(sources[0]["streams"][1]["language"], "eng");
	assert_eq!(sources[1]["label"], "B");
	assert_eq!(sources[1]["offsetSeconds"], 0.5);
	assert_eq!(sources[1]["streams"].as_array().unwrap().len(), 3);
	assert_eq!(sources[1]["streams"][1]["title"], "French");
	assert_eq!(sources[2]["label"], "C");
	assert!(int(&sources[0]["durationMs"]) > 1500, "{preview}");

	// The output tracks: where each comes from and what it will carry.
	assert_eq!(origins(&preview), ["A:#0", "B:#1", "C:#0"]);
	let tracks = preview["tracks"].as_array().unwrap();
	assert_eq!(tracks[0]["stream"]["type"], "STREAM_TYPE_VIDEO");
	assert_eq!(tracks[0]["stream"]["codec"], "h264");
	assert_ne!(tracks[0]["edited"], true);
	assert_eq!(tracks[1]["source"], 1);
	assert_eq!(tracks[1]["stream"]["codec"], "aac");
	assert_eq!(tracks[1]["language"], "fre", "carried over from B");
	assert_eq!(tracks[1]["title"], "Atmos", "set by the track");
	assert_eq!(tracks[1]["default"], true);
	assert_eq!(tracks[1]["edited"], true);
	assert_eq!(tracks[2]["position"], 2);
	assert_eq!(tracks[2]["stream"]["codec"], "subrip");
	assert_eq!(tracks[2]["language"], "eng");
	assert_eq!(tracks[2]["forced"], true);
	assert_ne!(tracks[2]["default"], true);

	// A lasts 2 s, B 4 s: a warning, no refusal.
	let warnings = preview["warnings"].as_array().unwrap();
	assert_eq!(warnings.len(), 1, "{preview}");
	let warning = warnings[0].as_str().unwrap();
	assert!(
		warning.starts_with("durations differ: A (a.mkv) 0:00:02.0")
			&& warning.contains("B (b.mkv) 0:00:04.0")
			&& warning.ends_with("s)"),
		"{warning}"
	);

	// The exact command. Whether B's audio already carries the default
	// flag depends on the muxer that wrote b.mkv, so that option may or
	// may not appear; the title and the subtitle's edits always do.
	let command = preview["command"].as_str().unwrap();
	let expected = format!(
		"-i {} -itsoffset 0.5 -i {} -i {} -map 0:0 -map 1:1 -map 2:0 -c copy -metadata:s:1 title=Atmos",
		root.a.display(),
		root.b.display(),
		root.srt.display()
	);
	assert!(command.contains(&expected), "{command}");
	assert!(
		command.ends_with(&format!(
			"-metadata:s:2 language=eng -metadata:s:2 title=English -disposition:2 forced {}",
			root.path().join("movie.mkv").display()
		)),
		"{command}"
	);
	let argv = preview["argv"].as_array().unwrap();
	assert!(argv.iter().any(|a| a == "-c"), "{preview}");
}

#[tokio::test]
async fn video_audio_and_subs_from_three_sources_make_one_mkv() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let operation = three_sources(&root);
	let (_, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();

	// The job runs the previewed command and the output holds the three
	// tracks with their metadata.
	let job = run_job(&router, operation).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	assert_eq!(job["name"], "movie.mkv");
	assert_eq!(job["command"], preview["command"]);
	let out = root.probe("movie.mkv").await.unwrap();
	assert_eq!(
		streams(&out),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Audio, "aac".into()),
			(CodecType::Subtitle, "subrip".into()),
		]
	);
	assert_eq!(out.streams[1].language(), Some("fre"));
	assert_eq!(
		out.streams[1].tags.get("title").map(String::as_str),
		Some("Atmos")
	);
	assert!(out.streams[1].disposition.default);
	assert_eq!(out.streams[2].language(), Some("eng"));
	assert_eq!(
		out.streams[2].tags.get("title").map(String::as_str),
		Some("English")
	);
	assert!(out.streams[2].disposition.forced);
}

#[tokio::test]
async fn subtitles_from_another_file_are_added_and_unused_sources_are_left_out() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let operation = json!({
		"sources": [{ "input": root.a }, { "input": root.b }, { "input": root.srt }],
		"tracks": [
			{ "source": 0, "streamIndex": 0 },
			{ "source": 0, "streamIndex": 1 },
			{ "source": 1, "streamIndex": 2 },
		],
		"output": root.path().join("out/a-with-subs.mkv"),
	});
	let (status, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{preview}");
	assert_eq!(origins(&preview), ["A:#0", "A:#1", "B:#2"]);
	assert_ne!(preview["sources"][2]["used"], true, "C adds nothing");
	// Only subtitles come from B: its duration is not compared.
	assert!(
		preview["warnings"].as_array().is_none_or(Vec::is_empty),
		"{preview}"
	);
	let command = preview["command"].as_str().unwrap();
	assert!(!command.contains("subs.srt"), "{command}");
	assert!(
		command.contains("-map 0:0 -map 0:1 -map 1:2 -c copy"),
		"{command}"
	);

	let job = run_job(&router, operation).await.unwrap();
	assert_eq!(job["state"], "JOB_STATE_SUCCEEDED", "{job}");
	let out = root.probe("out/a-with-subs.mkv").await.unwrap();
	assert_eq!(
		streams(&out),
		[
			(CodecType::Video, "h264".into()),
			(CodecType::Audio, "aac".into()),
			(CodecType::Subtitle, "subrip".into()),
		]
	);
	assert_eq!(out.streams[1].language(), Some("eng"));
	assert_eq!(out.streams[2].language(), Some("eng"));
}

#[tokio::test]
async fn a_container_that_cannot_hold_a_track_is_reported_before_running() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let operation = json!({
		"sources": [{ "input": root.a }, { "input": root.srt }],
		"tracks": [{ "source": 0, "streamIndex": 0 }, { "source": 1, "streamIndex": 0 }],
		"output": "movie.mp4",
	});
	let (status, preview) = post_json(&router, PREVIEW, json!({ "operation": operation }))
		.await
		.unwrap();
	assert_eq!(status, StatusCode::OK, "{preview}");
	assert_eq!(preview["container"], "MP4");
	assert_ne!(preview["canRun"], true, "{preview}");
	let issues = preview["issues"].as_array().unwrap();
	assert_eq!(issues.len(), 1, "{preview}");
	assert_eq!(issues[0]["track"], 1);
	assert_eq!(issues[0]["blocking"], true);
	assert_eq!(
		issues[0]["message"],
		"MP4 cannot hold subrip subtitles (track 2, B:#0); remove the track or choose MKV"
	);

	let (status, json) = post_json(&router, SUBMIT, json!({ "combine": operation }))
		.await
		.unwrap();
	assert_ne!(status, StatusCode::OK);
	assert_eq!(json["code"], "failed_precondition", "{json}");
	assert_eq!(
		json["message"],
		"the MP4 output cannot hold every track: MP4 cannot hold subrip subtitles (track 2, B:#0); remove the track or choose MKV"
	);
	assert!(!root.path().join("movie.mp4").exists());
}

#[tokio::test]
async fn a_source_can_be_a_file_id() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let (_, roots) = post_json(&router, LIST_FOLDER, json!({})).await.unwrap();
	let folder = roots["entries"][0]["id"].as_str().unwrap();
	let (_, listing) = post_json(&router, LIST_FOLDER, json!({ "folderId": folder }))
		.await
		.unwrap();
	let id = listing["entries"]
		.as_array()
		.unwrap()
		.iter()
		.find(|e| e["name"] == "b.mkv")
		.unwrap()["id"]
		.as_str()
		.unwrap()
		.to_owned();
	let (status, preview) = post_json(
		&router,
		PREVIEW,
		json!({ "operation": {
			"sources": [{ "input": root.a }, { "input": id }],
			"tracks": [{ "source": 0, "streamIndex": 0 }, { "source": 1, "streamIndex": 1 }],
			"output": "by-id.mkv",
		} }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{preview}");
	assert_eq!(preview["sources"][1]["path"], root.b.display().to_string());
	assert_eq!(origins(&preview), ["A:#0", "B:#1"]);
}

#[tokio::test]
async fn combine_requests_are_validated() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let op = |sources: Value, tracks: Value, output: &str| json!({ "sources": sources, "tracks": tracks, "output": output });
	let a = json!({ "input": root.a });
	let b = json!({ "input": root.b });
	let cases = [
		(
			json!({}),
			PREVIEW,
			"invalid_argument",
			"an operation is required".to_owned(),
		),
		(
			json!({ "operation": op(json!([]), json!([]), "x.mkv") }),
			PREVIEW,
			"invalid_argument",
			"no source file is given".to_owned(),
		),
		(
			json!({ "operation": op(json!([a]), json!([]), "x.mkv") }),
			PREVIEW,
			"invalid_argument",
			"no track is added to the output".to_owned(),
		),
		(
			json!({ "operation": op(json!([a, b]), json!([{ "source": 1, "streamIndex": 7 }]), "x.mkv") }),
			PREVIEW,
			"invalid_argument",
			"source B (b.mkv) has no stream 7".to_owned(),
		),
		(
			json!({ "operation": op(json!([a]), json!([{ "source": 2, "streamIndex": 0 }]), "x.mkv") }),
			PREVIEW,
			"invalid_argument",
			"track 1 names source 2, but there are only 1 sources".to_owned(),
		),
		(
			json!({ "operation": op(json!([a, b]), json!([{ "source": 1, "streamIndex": 2 }, { "source": 1, "streamIndex": 2 }]), "x.mkv") }),
			PREVIEW,
			"invalid_argument",
			"B:#2 is added twice".to_owned(),
		),
		(
			json!({ "operation": op(json!([a]), json!([{ "source": 0, "streamIndex": 0 }]), "x.avi") }),
			PREVIEW,
			"invalid_argument",
			"the output's extension picks the container; use one of .mkv, .mka, .mks, .mp4, .m4v, .m4a, .mov, .webm".to_owned(),
		),
		(
			json!({ "operation": op(json!([a]), json!([{ "source": 0, "streamIndex": 0 }]), "sub/x.mkv") }),
			PREVIEW,
			"invalid_argument",
			"output must be an absolute path or a bare file name: sub/x.mkv".to_owned(),
		),
		(
			json!({ "operation": op(json!([{ "input": "/nowhere/clip.mkv" }]), json!([{ "source": 0, "streamIndex": 0 }]), "x.mkv") }),
			PREVIEW,
			"not_found",
			"input file not found: /nowhere/clip.mkv".to_owned(),
		),
		(
			json!({ "combine": op(json!([a]), json!([{ "source": 0, "streamIndex": 0 }]), "b.mkv") }),
			SUBMIT,
			"already_exists",
			format!("output file already exists: {}", root.b.display()),
		),
	];
	for (body, path, code, message) in cases {
		let (status, json) = post_json(&router, path, body.clone()).await.unwrap();
		assert_ne!(status, StatusCode::OK, "{body}");
		assert_eq!(json["code"], code, "{body}: {json}");
		assert_eq!(json["message"], message, "{body}: {json}");
	}
}
