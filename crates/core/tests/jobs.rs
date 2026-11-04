//! The job queue with the real ffmpeg from `PATH`: the state machine,
//! cancellation, concurrency, shutdown and resumable watch streams.

use std::path::Path;
use std::time::Duration;

use muxmultitool_core::ffmpeg::{FfmpegCommand, Runner, Tools, operations};
use muxmultitool_core::jobs::{
	EventKind, Job, JobId, JobSpec, JobState, Queue, QueueConfig, QueueError, Watch, WatchItem,
};

/// A job that finishes in well under a second.
fn quick(name: &str) -> JobSpec {
	JobSpec {
		name: name.to_owned(),
		command: FfmpegCommand::new()
			.option("-f", "lavfi")
			.input("testsrc2=duration=1:size=64x64:rate=10")
			.option("-f", "null")
			.output("-"),
		expected_duration: Some(Duration::from_secs(1)),
		preparation: Vec::new(),
		scratch_dir: None,
		outputs: Vec::new(),
	}
}

/// A job that would run for ten minutes at real-time pace. `marker` ends up
/// in its argument list, so the process can be found.
fn endless(marker: &str) -> JobSpec {
	JobSpec {
		name: marker.to_owned(),
		command: FfmpegCommand::new()
			.flag("-re")
			.option("-f", "lavfi")
			.input("testsrc2=duration=600:size=64x64:rate=10")
			.option("-metadata", format!("comment={marker}"))
			.option("-f", "null")
			.output("-"),
		expected_duration: Some(Duration::from_secs(600)),
		preparation: Vec::new(),
		scratch_dir: None,
		outputs: Vec::new(),
	}
}

/// Whether any process on this machine has an argument ending in `marker`.
/// Linux only: it reads `/proc`, the one way to look without spawning.
fn process_with_argument_exists(marker: &str) -> bool {
	if !cfg!(target_os = "linux") {
		return false;
	}
	std::fs::read_dir("/proc")
		.into_iter()
		.flatten()
		.flatten()
		.filter_map(|entry| std::fs::read(entry.path().join("cmdline")).ok())
		.any(|cmdline| {
			cmdline
				.split(|b| *b == 0)
				.any(|arg| arg.ends_with(marker.as_bytes()))
		})
}

/// Reads a watch to its end. `None` when it takes longer than 30 s.
async fn collect(mut watch: Watch) -> Option<Vec<WatchItem>> {
	tokio::time::timeout(Duration::from_secs(30), async {
		let mut items = Vec::new();
		while let Some(item) = watch.next().await {
			items.push(item);
		}
		items
	})
	.await
	.ok()
}

/// Reads a queue watch, which never ends by itself, until it has delivered
/// the event that ends `job`. `None` when that takes longer than 30 s.
async fn collect_until_job_ends(mut watch: Watch, job: &JobId) -> Option<Vec<WatchItem>> {
	tokio::time::timeout(Duration::from_secs(30), async {
		let mut items = Vec::new();
		while let Some(item) = watch.next().await {
			let ends_job = matches!(&item, WatchItem::Event(e) if &e.job == job && e.is_terminal());
			items.push(item);
			if ends_job {
				break;
			}
		}
		items
	})
	.await
	.ok()
}

/// Reads `watch` until ffmpeg reported progress, which proves the process
/// is up. `None` when the watch ends first or nothing comes within 30 s.
async fn wait_for_progress(mut watch: Watch) -> Option<()> {
	tokio::time::timeout(Duration::from_secs(30), async {
		loop {
			match watch.next().await {
				Some(WatchItem::Event(e)) if matches!(e.kind, EventKind::Progress(_)) => {
					return Some(());
				}
				Some(_) => {}
				None => return None,
			}
		}
	})
	.await
	.ok()
	.flatten()
}

/// The states reported by the events in `items`, in order.
fn states(items: &[WatchItem]) -> Vec<JobState> {
	items
		.iter()
		.filter_map(|item| match item {
			WatchItem::Event(event) => match event.kind {
				EventKind::State { state, .. } => Some(state),
				_ => None,
			},
			WatchItem::Snapshot(_) => None,
		})
		.collect()
}

/// Waits until `id` reaches a terminal state and returns the job. `None`
/// for an unknown id or when that takes longer than 30 s.
async fn wait_for_end(queue: &Queue, id: &JobId) -> Option<Job> {
	let mut watch = queue.watch_job(id, 0).ok()?;
	tokio::time::timeout(Duration::from_secs(30), async {
		while watch.next().await.is_some() {}
	})
	.await
	.ok()?;
	queue.get(id)
}

#[tokio::test]
async fn a_job_runs_through_its_states_and_shows_its_command() {
	let queue = Queue::new(Runner::default(), QueueConfig::default());
	let job = queue.submit(quick("one")).unwrap();
	assert_eq!(job.argv[0], "ffmpeg");
	assert!(
		job.command_line
			.starts_with("ffmpeg -hide_banner -nostdin -nostats -progress pipe:1 -f lavfi"),
		"{}",
		job.command_line
	);
	// Started at once: the slot was free.
	assert_eq!(job.state, JobState::Running);
	assert!(job.started.is_some());

	let items = collect(queue.watch_job(&job.id, 0).unwrap()).await.unwrap();
	let WatchItem::Snapshot(snapshot) = &items[0] else {
		panic!("first item is the snapshot: {:?}", items[0]);
	};
	assert_eq!(snapshot.jobs.len(), 1);
	assert_eq!(snapshot.jobs[0].command_line, job.command_line);
	assert_eq!(states(&items), vec![JobState::Succeeded]);

	let progress: Vec<_> = items
		.iter()
		.filter_map(|item| match item {
			WatchItem::Event(event) => match &event.kind {
				EventKind::Progress(p) => Some(p.clone()),
				_ => None,
			},
			WatchItem::Snapshot(_) => None,
		})
		.collect();
	let last = progress.last().expect("progress was reported");
	assert!(last.report.end, "{last:?}");
	assert_eq!(last.percent, Some(100.0));
	assert_eq!(last.eta, Some(Duration::ZERO));

	let logged = items.iter().any(
		|item| matches!(item, WatchItem::Event(event) if matches!(event.kind, EventKind::Log(_))),
	);
	assert!(logged, "ffmpeg's stderr lines were forwarded");

	let done = queue.get(&job.id).unwrap();
	assert_eq!(done.state, JobState::Succeeded);
	assert!(done.finished.is_some());
	assert!(done.error.is_none());
	assert_ne!(done.log.len(), 0);
	assert_eq!(queue.list().len(), 1);

	// Ids increase in event order and the snapshot names the latest.
	let ids: Vec<u64> = items.iter().map(WatchItem::event_id).collect();
	assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
}

#[tokio::test]
async fn a_job_of_several_commands_runs_them_in_order_and_cleans_up() {
	let dir = tempfile::tempdir().unwrap();
	let scratch = dir.path().join("scratch");
	std::fs::create_dir_all(&scratch).unwrap();
	let piece = scratch.join("piece.mkv");
	let output = dir.path().join("out.mkv");
	let queue = Queue::new(Runner::default(), QueueConfig::default());
	// The first command writes the piece, the second reads it.
	let write = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("testsrc2=duration=1:size=64x64:rate=10")
		.output(&piece);
	let read = FfmpegCommand::new()
		.input(&piece)
		.option("-c", "copy")
		.output(&output);
	let job = queue
		.submit(JobSpec {
			name: "two steps".into(),
			command: read.clone(),
			expected_duration: Some(Duration::from_secs(1)),
			preparation: vec![write.clone()],
			scratch_dir: Some(scratch.clone()),
			outputs: vec![output.clone()],
		})
		.unwrap();
	assert!(
		job.command_line.contains(" && ffmpeg "),
		"{}",
		job.command_line
	);
	assert_eq!(job.argv.iter().filter(|a| *a == "&&").count(), 1);
	let done = wait_for_end(&queue, &job.id).await.unwrap();
	assert_eq!(done.state, JobState::Succeeded, "{done:?}");
	assert!(output.is_file(), "a succeeded job keeps its output");
	assert!(!scratch.exists(), "the scratch folder is removed");
	// The first command's share of the percentage ended at 50.
	let items = collect(queue.watch_job(&job.id, 0).unwrap()).await.unwrap();
	let _ = items;

	// A failing first command ends the job there: the second never runs.
	std::fs::create_dir_all(&scratch).unwrap();
	let output2 = dir.path().join("out2.mkv");
	let job = queue
		.submit(JobSpec {
			name: "broken first".into(),
			command: FfmpegCommand::new()
				.input(&piece)
				.option("-c", "copy")
				.output(&output2),
			expected_duration: None,
			preparation: vec![
				FfmpegCommand::new()
					.input(dir.path().join("missing.mkv"))
					.output(&piece),
			],
			scratch_dir: Some(scratch.clone()),
			outputs: Vec::new(),
		})
		.unwrap();
	let done = wait_for_end(&queue, &job.id).await.unwrap();
	assert_eq!(done.state, JobState::Failed, "{done:?}");
	assert!(!output2.exists());
	assert!(!scratch.exists());
}

#[tokio::test]
async fn a_failed_job_explains_itself_and_keeps_the_ffmpeg_output() {
	let dir = tempfile::tempdir().unwrap();
	let queue = Queue::new(Runner::default(), QueueConfig::default());
	let job = queue
		.submit(JobSpec {
			name: "broken".into(),
			command: operations::remux(
				&dir.path().join("missing.mkv"),
				&dir.path().join("out.mp4"),
			),
			expected_duration: None,
			preparation: Vec::new(),
			scratch_dir: None,
			outputs: Vec::new(),
		})
		.unwrap();

	let done = wait_for_end(&queue, &job.id).await.unwrap();
	assert_eq!(done.state, JobState::Failed);
	let error = done.error.expect("a failed job has an error");
	assert_eq!(error.message, "the input file was not found");
	assert!(
		error.ffmpeg_output.contains("missing.mkv"),
		"{}",
		error.ffmpeg_output
	);
	assert!(done.log.iter().any(|line| line.contains("missing.mkv")));

	// The terminal event carries the same error.
	let items = collect(queue.watch_job(&job.id, 0).unwrap()).await.unwrap();
	assert_eq!(
		items.len(),
		1,
		"a finished job is only a snapshot: {items:?}"
	);
}

#[tokio::test]
async fn a_missing_ffmpeg_fails_the_job_instead_of_the_server() {
	let runner = Runner::new(Tools::in_dir(Path::new("/nonexistent/mmt")));
	let queue = Queue::new(runner, QueueConfig::default());
	let job = queue.submit(quick("no ffmpeg")).unwrap();
	let done = wait_for_end(&queue, &job.id).await.unwrap();
	assert_eq!(done.state, JobState::Failed);
	let error = done.error.unwrap();
	assert!(
		error.message.starts_with("could not start ffmpeg"),
		"{}",
		error.message
	);
	assert_eq!(error.ffmpeg_output, "");
}

#[tokio::test]
async fn concurrency_limits_how_many_jobs_run() {
	let pid = std::process::id();
	let first_marker = format!("mmt-jobs-first-{pid}");
	let second_marker = format!("mmt-jobs-second-{pid}");
	let queue = Queue::new(Runner::default(), QueueConfig { concurrency: 1 });
	let first = queue.submit(endless(&first_marker)).unwrap();
	let second = queue.submit(endless(&second_marker)).unwrap();
	assert_eq!(first.state, JobState::Running);
	assert_eq!(second.state, JobState::Queued);
	assert!(second.started.is_none());

	// Stopping the first frees the slot for the second.
	let cancelled = queue.cancel(&first.id).unwrap();
	assert_eq!(
		cancelled.state,
		JobState::Running,
		"cancelled once ffmpeg is gone"
	);
	let first = wait_for_end(&queue, &first.id).await.unwrap();
	assert_eq!(first.state, JobState::Cancelled);
	assert!(
		!process_with_argument_exists(&first_marker),
		"ffmpeg was reaped"
	);

	let second = queue.get(&second.id).unwrap();
	assert_eq!(second.state, JobState::Running);
	// The second runs for real.
	wait_for_progress(queue.watch_job(&second.id, 0).unwrap())
		.await
		.unwrap();
	if cfg!(target_os = "linux") {
		assert!(process_with_argument_exists(&second_marker));
	}

	queue.cancel(&second.id).unwrap();
	let second = wait_for_end(&queue, &second.id).await.unwrap();
	assert_eq!(second.state, JobState::Cancelled);
	assert!(
		!process_with_argument_exists(&second_marker),
		"ffmpeg was reaped"
	);
}

#[tokio::test]
async fn cancelling_a_queued_job_never_starts_it() {
	let marker = format!("mmt-jobs-queued-{}", std::process::id());
	let queue = Queue::new(Runner::default(), QueueConfig { concurrency: 1 });
	let blocker = queue.submit(endless(&marker)).unwrap();
	let waiting = queue.submit(quick("waiting")).unwrap();
	assert_eq!(waiting.state, JobState::Queued);

	let cancelled = queue.cancel(&waiting.id).unwrap();
	assert_eq!(cancelled.state, JobState::Cancelled);
	assert!(cancelled.started.is_none());
	assert!(cancelled.finished.is_some());

	// Terminal jobs and unknown ids are errors, not silent no-ops.
	assert_eq!(
		queue.cancel(&waiting.id),
		Err(QueueError::Finished(
			waiting.id.clone(),
			JobState::Cancelled
		))
	);
	let unknown = JobId::from("nope");
	assert_eq!(
		queue.cancel(&unknown),
		Err(QueueError::NotFound(unknown.clone()))
	);
	assert!(matches!(
		queue.watch_job(&unknown, 0),
		Err(QueueError::NotFound(_))
	));
	assert_eq!(queue.get(&unknown), None);

	queue.cancel(&blocker.id).unwrap();
	wait_for_end(&queue, &blocker.id).await.unwrap();
	assert!(!process_with_argument_exists(&marker));
}

#[tokio::test]
async fn a_watch_resumes_from_the_last_event_id() {
	let queue = Queue::new(Runner::default(), QueueConfig::default());
	let job = queue.submit(quick("resume")).unwrap();
	let all = collect_until_job_ends(queue.watch_queue(0), &job.id)
		.await
		.unwrap();
	let WatchItem::Snapshot(first) = &all[0] else {
		panic!("snapshot first");
	};
	assert_eq!(first.jobs.len(), 1);
	let events: Vec<_> = all[1..]
		.iter()
		.map(|item| match item {
			WatchItem::Event(event) => event.clone(),
			WatchItem::Snapshot(_) => panic!("only one snapshot"),
		})
		.collect();
	assert!(events.len() >= 3, "{events:?}");

	// Resume after the second event: the rest comes back verbatim, then
	// the stream ends because the job is over.
	let after = events[1].id;
	let resumed = collect(queue.watch_job(&job.id, after).unwrap())
		.await
		.unwrap();
	let resumed_ids: Vec<u64> = resumed.iter().map(WatchItem::event_id).collect();
	let expected: Vec<u64> = events[2..].iter().map(|e| e.id).collect();
	assert_eq!(resumed_ids, expected);
	assert!(
		resumed
			.iter()
			.all(|item| matches!(item, WatchItem::Event(_)))
	);

	// A client that saw everything gets nothing more.
	let latest = events.last().unwrap().id;
	let caught_up_job = collect(queue.watch_job(&job.id, latest).unwrap())
		.await
		.unwrap();
	assert!(matches!(&caught_up_job[..], []), "{caught_up_job:?}");
	let mut caught_up = queue.watch_queue(latest);
	let nothing = tokio::time::timeout(Duration::from_millis(200), caught_up.next()).await;
	assert!(
		nothing.is_err(),
		"a caught-up queue watch waits for new events: {nothing:?}"
	);

	// An id the server never handed out cannot be resumed: snapshot again.
	let fresh = collect(queue.watch_job(&job.id, latest + 100).unwrap())
		.await
		.unwrap();
	assert!(
		matches!(&fresh[..], [WatchItem::Snapshot(s)] if s.event_id == latest),
		"{fresh:?}"
	);
}

#[tokio::test]
async fn shutdown_stops_every_job_and_ends_every_watch() {
	let pid = std::process::id();
	let running_marker = format!("mmt-jobs-shutdown-running-{pid}");
	let queued_marker = format!("mmt-jobs-shutdown-queued-{pid}");
	let queue = Queue::new(Runner::default(), QueueConfig { concurrency: 1 });
	let running = queue.submit(endless(&running_marker)).unwrap();
	let queued = queue.submit(endless(&queued_marker)).unwrap();
	wait_for_progress(queue.watch_job(&running.id, 0).unwrap())
		.await
		.unwrap();
	if cfg!(target_os = "linux") {
		assert!(process_with_argument_exists(&running_marker));
	}

	let watch = tokio::spawn(collect(queue.watch_queue(0)));

	tokio::time::timeout(Duration::from_secs(30), queue.shutdown())
		.await
		.expect("shutdown finishes");

	assert_eq!(queue.get(&running.id).unwrap().state, JobState::Cancelled);
	assert_eq!(queue.get(&queued.id).unwrap().state, JobState::Cancelled);
	assert!(
		!process_with_argument_exists(&running_marker),
		"ffmpeg was reaped"
	);
	assert!(
		!process_with_argument_exists(&queued_marker),
		"never started"
	);

	let items = watch.await.unwrap().unwrap();
	let cancelled = states(&items)
		.into_iter()
		.filter(|s| *s == JobState::Cancelled)
		.count();
	assert_eq!(
		cancelled, 2,
		"the watch saw both ends before closing: {items:?}"
	);

	assert_eq!(
		queue.submit(quick("late")).unwrap_err(),
		QueueError::ShuttingDown
	);
	tokio::time::timeout(Duration::from_secs(1), queue.closed())
		.await
		.expect("closed resolves after shutdown");
}

/// A job that writes `output` for ten minutes at real-time pace, so there
/// is a partial file to find while it runs. `marker` ends up in its
/// argument list.
fn endless_to_file(marker: &str, output: &Path) -> JobSpec {
	JobSpec {
		name: marker.to_owned(),
		command: FfmpegCommand::new()
			.flag("-re")
			.option("-f", "lavfi")
			.input("testsrc2=duration=600:size=64x64:rate=10")
			.option("-metadata", format!("comment={marker}"))
			.option("-f", "mpegts")
			.output(output),
		expected_duration: Some(Duration::from_secs(600)),
		preparation: Vec::new(),
		scratch_dir: None,
		outputs: vec![output.to_path_buf()],
	}
}

#[tokio::test]
async fn a_cancelled_job_removes_the_output_it_created() {
	let dir = tempfile::tempdir().unwrap();
	let output = dir.path().join("partial.ts");
	let marker = format!("mmt-jobs-cancel-output-{}", std::process::id());
	let queue = Queue::new(Runner::default(), QueueConfig::default());
	let job = queue.submit(endless_to_file(&marker, &output)).unwrap();
	wait_for_progress(queue.watch_job(&job.id, 0).unwrap())
		.await
		.unwrap();
	assert!(output.exists(), "ffmpeg has started writing");

	queue.cancel(&job.id).unwrap();
	let done = wait_for_end(&queue, &job.id).await.unwrap();
	assert_eq!(done.state, JobState::Cancelled);
	assert!(!output.exists(), "the partial output is removed");
}

#[tokio::test]
async fn shutdown_removes_the_outputs_of_the_jobs_it_cancels() {
	let dir = tempfile::tempdir().unwrap();
	let output = dir.path().join("partial.ts");
	let marker = format!("mmt-jobs-shutdown-output-{}", std::process::id());
	let queue = Queue::new(Runner::default(), QueueConfig::default());
	let job = queue.submit(endless_to_file(&marker, &output)).unwrap();
	wait_for_progress(queue.watch_job(&job.id, 0).unwrap())
		.await
		.unwrap();
	assert!(output.exists());

	tokio::time::timeout(Duration::from_secs(30), queue.shutdown())
		.await
		.expect("shutdown finishes");
	assert_eq!(queue.get(&job.id).unwrap().state, JobState::Cancelled);
	assert!(!output.exists(), "the partial output is removed");
}

#[tokio::test]
async fn a_failed_job_removes_the_outputs_it_created_and_keeps_the_rest() {
	let dir = tempfile::tempdir().unwrap();
	let written = dir.path().join("written.mkv");
	let never = dir.path().join("never.mkv");
	let existing = dir.path().join("existing.mkv");
	std::fs::write(&existing, "keep me").unwrap();
	let input = dir.path().join("input.mkv");
	let queue = Queue::new(Runner::default(), QueueConfig::default());
	// The first step writes a file; the second fails on a missing input.
	// Both outputs are listed, as is a file that was there before and an
	// output the job never reached.
	let job = queue
		.submit(JobSpec {
			name: "fails late".into(),
			command: FfmpegCommand::new()
				.input(dir.path().join("missing.mkv"))
				.output(&never),
			expected_duration: None,
			preparation: vec![
				FfmpegCommand::new()
					.option("-f", "lavfi")
					.input("testsrc2=duration=1:size=64x64:rate=10")
					.output(&written),
			],
			scratch_dir: None,
			outputs: vec![
				written.clone(),
				never.clone(),
				existing.clone(),
				input.clone(),
			],
		})
		.unwrap();
	let done = wait_for_end(&queue, &job.id).await.unwrap();
	assert_eq!(done.state, JobState::Failed, "{done:?}");
	assert!(
		!written.exists(),
		"the output of the finished step is removed"
	);
	assert!(!never.exists());
	assert_eq!(
		std::fs::read(&existing).unwrap(),
		b"keep me",
		"a file that existed before the job is left alone"
	);
	assert!(!input.exists());
}

#[tokio::test]
async fn an_output_that_exists_before_the_job_is_never_removed() {
	let dir = tempfile::tempdir().unwrap();
	let output = dir.path().join("out.mkv");
	std::fs::write(&output, "not a movie").unwrap();
	let queue = Queue::new(Runner::default(), QueueConfig::default());
	// ffmpeg refuses to overwrite without `-y`: the job fails at once.
	let job = queue
		.submit(JobSpec {
			name: "refused".into(),
			command: FfmpegCommand::new()
				.option("-f", "lavfi")
				.input("testsrc2=duration=1:size=64x64:rate=10")
				.output(&output),
			expected_duration: None,
			preparation: Vec::new(),
			scratch_dir: None,
			outputs: vec![output.clone()],
		})
		.unwrap();
	let done = wait_for_end(&queue, &job.id).await.unwrap();
	assert_eq!(done.state, JobState::Failed, "{done:?}");
	assert_eq!(
		done.error.unwrap().message,
		"the output file already exists"
	);
	assert_eq!(std::fs::read(&output).unwrap(), b"not a movie");
}
