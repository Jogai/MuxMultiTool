//! The jobs service end to end: the generated client over a socket, request
//! validation, the Connect streaming envelope a browser reads, and a clean
//! stop with a watch stream open. Needs ffmpeg and ffprobe on `PATH`.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use connectrpc::client::{ClientConfig, HttpClient};
use connectrpc::{CodecFormat, ErrorCode};
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner};
use muxmultitool_proto::connect::muxmultitool::jobs::v1::JobsServiceClient;
use muxmultitool_proto::proto::muxmultitool::jobs::v1::__buffa::oneof::job_event::Event;
use muxmultitool_proto::proto::muxmultitool::jobs::v1::__buffa::oneof::watch_job_response::Message;
use muxmultitool_proto::proto::muxmultitool::jobs::v1::{
	CancelJobRequest, GetJobRequest, Job, JobState, ListJobsRequest, RemuxOperation,
	SubmitJobRequest, WatchJobRequest, WatchJobResponse, WatchQueueRequest, WatchQueueResponse,
};
use muxmultitool_server::{Config, Server, app};
use tower::ServiceExt;

const SUBMIT: &str = "/rpc/muxmultitool.jobs.v1.JobsService/SubmitJob";
const GET: &str = "/rpc/muxmultitool.jobs.v1.JobsService/GetJob";
const WATCH_JOB: &str = "/rpc/muxmultitool.jobs.v1.JobsService/WatchJob";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A media root with a two-second H.264 + AAC clip in it.
struct MediaRoot {
	dir: tempfile::TempDir,
	clip: PathBuf,
}

impl MediaRoot {
	async fn new() -> Result<Self> {
		let dir = tempfile::tempdir()?;
		let clip = dir.path().join("clip.mp4");
		let cmd = FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("testsrc2=duration=2:size=160x120:rate=25")
			.option("-f", "lavfi")
			.input("sine=frequency=440:duration=2")
			.option("-c:v", "libx264")
			.option("-preset", "ultrafast")
			.option("-c:a", "aac")
			.output(&clip);
		Runner::default()
			.run(&cmd, &Limits::timeout(Duration::from_secs(60)), None)
			.await?;
		Ok(Self { dir, clip })
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

	fn remux(&self, output: &str) -> SubmitJobRequest {
		SubmitJobRequest {
			operation: Some(
				RemuxOperation {
					input: self.clip.to_string_lossy().into_owned(),
					output: self.path().join(output).to_string_lossy().into_owned(),
					..Default::default()
				}
				.into(),
			),
			..Default::default()
		}
	}
}

/// A server on a random port, with its stop handle.
struct Running {
	client: JobsServiceClient<HttpClient>,
	stop: tokio::sync::oneshot::Sender<()>,
	task: tokio::task::JoinHandle<std::io::Result<()>>,
}

async fn start(config: &Config, format: CodecFormat) -> Result<Running> {
	let server = Server::bind(config).await?;
	let addr = server.local_addr()?;
	let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
	let task = tokio::spawn(server.run(async {
		stopped.await.ok();
	}));
	let base: http::Uri = format!("http://{addr}/rpc").parse()?;
	let client = JobsServiceClient::new(
		HttpClient::plaintext(),
		ClientConfig::new(base).with_codec_format(format),
	);
	Ok(Running { client, stop, task })
}

fn state_of(job: &Job) -> i32 {
	job.state.to_i32()
}

/// Sends a JSON request to the in-process router and returns the status
/// and the decoded body.
async fn post_json(
	router: &axum::Router,
	path: &str,
	body: serde_json::Value,
) -> Result<(StatusCode, serde_json::Value)> {
	let request = Request::post(path)
		.header(header::CONTENT_TYPE, "application/json")
		.body(Body::from(body.to_string()))?;
	let response = router.clone().oneshot(request).await?;
	let (parts, body) = response.into_parts();
	let body = body.collect().await?.to_bytes();
	Ok((parts.status, serde_json::from_slice(&body)?))
}

/// Reads a job watch to its end.
async fn watch_job_to_end(
	client: &JobsServiceClient<HttpClient>,
	job_id: &str,
	after_event_id: u64,
) -> Result<Vec<WatchJobResponse>> {
	let mut stream = client
		.watch_job(WatchJobRequest {
			job_id: job_id.to_owned(),
			after_event_id,
			..Default::default()
		})
		.await?;
	let mut messages = Vec::new();
	while let Some(message) = stream.message::<WatchJobResponse>().await? {
		messages.push(message.to_owned_message());
	}
	Ok(messages)
}

/// Splits a Connect streaming body into its envelopes: `(flags, payload)`.
fn envelopes(body: &[u8]) -> Vec<(u8, serde_json::Value)> {
	let mut frames = Vec::new();
	let mut rest = body;
	while rest.len() >= 5 {
		let flags = rest[0];
		let len = u32::from_be_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
		let payload = &rest[5..5 + len];
		frames.push((flags, serde_json::from_slice(payload).unwrap_or_default()));
		rest = &rest[5 + len..];
	}
	frames
}

#[tokio::test]
async fn a_job_is_submitted_watched_and_listed_over_the_generated_client() {
	let root = MediaRoot::new().await.unwrap();
	let running = start(&root.config(), CodecFormat::Json).await.unwrap();
	let client = &running.client;

	let submitted = client
		.submit_job(root.remux("remuxed.mkv"))
		.await
		.unwrap()
		.into_owned();
	let job = submitted.job.into_option().expect("the job is returned");
	assert_eq!(job.name, "remuxed.mkv");
	assert!(
		state_of(&job) == JobState::Queued as i32 || state_of(&job) == JobState::Running as i32,
		"{job:?}"
	);
	// The exact command is known before the run.
	assert!(job.command.contains("-progress pipe:1"), "{}", job.command);
	assert!(job.command.contains("remuxed.mkv"), "{}", job.command);
	assert_eq!(job.argv[0], "ffmpeg");
	assert!(job.argv.contains(&"copy".to_owned()), "{:?}", job.argv);
	assert!(job.created_unix_ms > 0);

	// Snapshot first, then the events up to the end.
	let messages = watch_job_to_end(client, &job.id, 0).await.unwrap();
	let Some(Message::Snapshot(snapshot)) = &messages[0].message else {
		panic!("first message is a snapshot: {:?}", messages[0]);
	};
	assert_eq!(snapshot.job.as_option().unwrap().command, job.command);
	let mut saw_progress = false;
	let mut saw_log = false;
	let mut last_state = None;
	let mut last_id = snapshot.event_id;
	for message in &messages[1..] {
		let Some(Message::Event(event)) = &message.message else {
			panic!("only the first message is a snapshot: {message:?}");
		};
		assert!(event.id > last_id, "ids increase: {event:?}");
		last_id = event.id;
		assert_eq!(event.job_id, job.id);
		match &event.event {
			Some(Event::Progress(p)) => {
				saw_progress = true;
				if p.percent.is_some() {
					assert!(p.eta_ms.is_some(), "{p:?}");
				}
			}
			Some(Event::LogLine(_)) => saw_log = true,
			Some(Event::State(change)) => last_state = Some(change.state.to_i32()),
			None => panic!("empty event"),
		}
	}
	assert!(saw_progress, "{messages:?}");
	assert!(saw_log, "{messages:?}");
	assert_eq!(last_state, Some(JobState::Succeeded as i32));

	let got = client
		.get_job(GetJobRequest {
			job_id: job.id.clone(),
			..Default::default()
		})
		.await
		.unwrap()
		.into_owned();
	let got = got.job.into_option().unwrap();
	assert_eq!(state_of(&got), JobState::Succeeded as i32);
	assert!(got.finished_unix_ms >= got.started_unix_ms);
	let progress = got.progress.as_option().expect("final progress");
	assert_eq!(progress.percent, Some(100.0));
	assert!(root.path().join("remuxed.mkv").is_file());

	let listed = client
		.list_jobs(ListJobsRequest::default())
		.await
		.unwrap()
		.into_owned();
	assert_eq!(listed.jobs.len(), 1);
	assert_eq!(listed.jobs[0].id, job.id);

	// A finished job cannot be cancelled, and unknown ids are not found.
	let error = client
		.cancel_job(CancelJobRequest {
			job_id: job.id.clone(),
			..Default::default()
		})
		.await
		.unwrap_err();
	assert_eq!(error.code, ErrorCode::FailedPrecondition, "{error}");
	let error = client
		.get_job(GetJobRequest {
			job_id: "nope".into(),
			..Default::default()
		})
		.await
		.unwrap_err();
	assert_eq!(error.code, ErrorCode::NotFound, "{error}");

	running.stop.send(()).unwrap();
	running.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_watch_resumes_from_the_last_event_id_over_the_generated_client() {
	let root = MediaRoot::new().await.unwrap();
	let running = start(&root.config(), CodecFormat::Json).await.unwrap();
	let client = &running.client;
	let job = client
		.submit_job(root.remux("resumed.mkv"))
		.await
		.unwrap()
		.into_owned()
		.job
		.into_option()
		.unwrap();
	let messages = watch_job_to_end(client, &job.id, 0).await.unwrap();
	let Some(Message::Snapshot(snapshot)) = &messages[0].message else {
		panic!("first message is a snapshot: {:?}", messages[0]);
	};
	let event_ids: Vec<u64> = messages[1..]
		.iter()
		.map(|m| match &m.message {
			Some(Message::Event(event)) => event.id,
			other => panic!("only the first message is a snapshot: {other:?}"),
		})
		.collect();
	assert!(event_ids.len() >= 3, "{event_ids:?}");

	// Resuming after the snapshot replays every event, without a snapshot.
	let replayed = watch_job_to_end(client, &job.id, snapshot.event_id)
		.await
		.unwrap();
	let replayed_ids: Vec<u64> = replayed
		.iter()
		.map(|m| match &m.message {
			Some(Message::Event(event)) => event.id,
			other => panic!("a resumed stream has no snapshot: {other:?}"),
		})
		.collect();
	assert_eq!(replayed_ids, event_ids);

	// Resuming halfway gets exactly the rest.
	let halfway = event_ids[event_ids.len() / 2];
	let rest = watch_job_to_end(client, &job.id, halfway).await.unwrap();
	assert_eq!(
		rest.len(),
		event_ids.iter().filter(|id| **id > halfway).count()
	);

	// After the last event nothing is missed and the stream ends at once.
	let last = *event_ids.last().unwrap();
	assert!(matches!(
		&watch_job_to_end(client, &job.id, last).await.unwrap()[..],
		[]
	));

	// An id the server never handed out gets a fresh snapshot.
	let fresh = watch_job_to_end(client, &job.id, last + 1000)
		.await
		.unwrap();
	assert!(
		matches!(&fresh[..], [WatchJobResponse { message: Some(Message::Snapshot(s)), .. }] if s.event_id == last),
		"{fresh:?}"
	);

	running.stop.send(()).unwrap();
	running.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_failed_job_reports_a_message_and_the_ffmpeg_output() {
	let root = MediaRoot::new().await.unwrap();
	let running = start(&root.config(), CodecFormat::Proto).await.unwrap();
	let client = &running.client;

	// A GIF cannot hold H.264 and AAC, so copying the streams fails.
	let job = client
		.submit_job(root.remux("broken.gif"))
		.await
		.unwrap()
		.into_owned()
		.job
		.into_option()
		.unwrap();
	let mut stream = client
		.watch_job(WatchJobRequest {
			job_id: job.id.clone(),
			..Default::default()
		})
		.await
		.unwrap();
	let mut failure = None;
	while let Some(message) = stream.message::<WatchJobResponse>().await.unwrap() {
		if let Some(Message::Event(event)) = message.to_owned_message().message
			&& let Some(Event::State(change)) = event.event
			&& change.state.to_i32() == JobState::Failed as i32
		{
			failure = change.error.into_option();
		}
	}
	let error = failure.expect("the failing state change carries the error");
	assert_ne!(error.message, "");
	assert!(
		!error.message.starts_with("ffmpeg exited"),
		"the container error is explained: {}",
		error.message
	);
	assert!(
		error.ffmpeg_output.contains("gif"),
		"{}",
		error.ffmpeg_output
	);

	let got = client
		.get_job(GetJobRequest {
			job_id: job.id,
			..Default::default()
		})
		.await
		.unwrap()
		.into_owned();
	let got = got.job.into_option().unwrap();
	assert_eq!(state_of(&got), JobState::Failed as i32);
	assert_eq!(got.error.as_option().unwrap().message, error.message);

	running.stop.send(()).unwrap();
	running.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn submit_validates_the_operation_and_its_paths() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());
	let clip = root.clip.to_string_lossy().into_owned();
	let outside = tempfile::tempdir().unwrap();
	let cases = [
		(
			serde_json::json!({}),
			"invalid_argument",
			"an operation is required",
		),
		(
			serde_json::json!({ "remux": { "input": "clip.mp4", "output": root.path().join("x.mkv") } }),
			"invalid_argument",
			"must be absolute",
		),
		(
			serde_json::json!({ "remux": { "input": root.path().join("missing.mp4"), "output": root.path().join("x.mkv") } }),
			"not_found",
			"input file not found",
		),
		(
			serde_json::json!({ "remux": { "input": clip, "output": clip } }),
			"already_exists",
			"output file already exists",
		),
		(
			serde_json::json!({ "remux": { "input": clip, "output": root.path().join("nope/x.mkv") } }),
			"invalid_argument",
			"output folder does not exist",
		),
		(
			serde_json::json!({ "remux": { "input": clip, "output": outside.path().join("x.mkv") } }),
			"permission_denied",
			"outside the media roots",
		),
	];
	for (body, code, message) in cases {
		let (status, json) = post_json(&router, SUBMIT, body.clone()).await.unwrap();
		assert_ne!(status, StatusCode::OK, "{body}");
		assert_eq!(json["code"], code, "{body}: {json}");
		assert!(
			json["message"]
				.as_str()
				.unwrap_or_default()
				.contains(message),
			"{body}: {json}"
		);
	}

	// Without media roots nothing can be submitted at all.
	let no_roots = app(&Config {
		media_roots: Vec::new(),
		..root.config()
	});
	let (status, json) = post_json(
		&no_roots,
		SUBMIT,
		serde_json::json!({ "remux": { "input": clip, "output": root.path().join("x.mkv") } }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::BAD_REQUEST);
	assert_eq!(json["code"], "failed_precondition");
}

#[tokio::test]
async fn watch_streams_use_the_connect_envelope_a_browser_reads() {
	let root = MediaRoot::new().await.unwrap();
	let router = app(&root.config());

	// Submit as the dev frontend does: JSON, and the job runs in the router's queue.
	let (status, json) = post_json(
		&router,
		SUBMIT,
		serde_json::json!({ "name": "envelope", "remux": { "input": root.clip, "output": root.path().join("env.mkv") } }),
	)
	.await
	.unwrap();
	assert_eq!(status, StatusCode::OK, "{json}");
	let id = json["job"]["id"].as_str().unwrap().to_owned();
	assert!(json["job"]["command"].as_str().unwrap().contains("env.mkv"));

	// Wait for the end so the stream is finite.
	let finished = tokio::time::timeout(Duration::from_secs(30), async {
		loop {
			let (_, json) = post_json(&router, GET, serde_json::json!({ "jobId": id }))
				.await
				.unwrap();
			if json["job"]["state"] == "JOB_STATE_SUCCEEDED" {
				return json;
			}
			tokio::time::sleep(Duration::from_millis(50)).await;
		}
	})
	.await
	.unwrap();
	assert!(
		finished["job"]["log"]
			.as_array()
			.is_some_and(|log| !log.is_empty())
	);

	// A streaming call: enveloped request, enveloped messages, end-stream frame.
	let request_body = serde_json::json!({ "jobId": id }).to_string();
	let mut body = vec![0u8];
	body.extend_from_slice(&u32::try_from(request_body.len()).unwrap().to_be_bytes());
	body.extend_from_slice(request_body.as_bytes());
	let request = Request::post(WATCH_JOB)
		.header(header::CONTENT_TYPE, "application/connect+json")
		.header("connect-protocol-version", "1")
		.body(Body::from(body))
		.unwrap();
	let response = router.clone().oneshot(request).await.unwrap();
	let (parts, body) = response.into_parts();
	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(
		parts.headers[header::CONTENT_TYPE],
		"application/connect+json"
	);
	let frames = envelopes(&body.collect().await.unwrap().to_bytes());
	let (first_flags, first) = &frames[0];
	assert_eq!(*first_flags, 0);
	assert_eq!(first["snapshot"]["job"]["id"], id);
	assert_eq!(first["snapshot"]["job"]["state"], "JOB_STATE_SUCCEEDED");
	assert!(
		first["snapshot"]["eventId"]
			.as_str()
			.is_some_and(|id| id != "0"),
		"{first}"
	);
	let (end_flags, end) = frames.last().unwrap();
	assert_eq!(*end_flags, 0b10, "end-stream flag");
	assert!(end.get("error").is_none(), "clean end: {end}");
	assert_eq!(
		frames.len(),
		2,
		"a finished job is just a snapshot: {frames:?}"
	);
}

#[tokio::test]
async fn stopping_the_server_ends_open_watch_streams() {
	let root = MediaRoot::new().await.unwrap();
	let running = start(&root.config(), CodecFormat::Json).await.unwrap();

	let mut stream = running
		.client
		.watch_queue(WatchQueueRequest::default())
		.await
		.unwrap();
	let first = stream
		.message::<WatchQueueResponse>()
		.await
		.unwrap()
		.expect("the queue snapshot");
	assert!(
		matches!(
			first.to_owned_message().message,
			Some(muxmultitool_proto::proto::muxmultitool::jobs::v1::__buffa::oneof::watch_queue_response::Message::Snapshot(_))
		),
		"{first:?}"
	);

	// The stop must not wait for the client to hang up.
	running.stop.send(()).unwrap();
	let stopped = tokio::time::timeout(Duration::from_secs(10), running.task)
		.await
		.expect("the server stops with a stream open");
	stopped.unwrap().unwrap();
	let next = tokio::time::timeout(
		Duration::from_secs(5),
		stream.message::<WatchQueueResponse>(),
	)
	.await
	.expect("the stream ends");
	assert!(matches!(next, Ok(None) | Err(_)), "{next:?}");
}
