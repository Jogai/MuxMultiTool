//! Timeline analyses against the real ffmpeg and ffprobe from `PATH`. Test
//! clips are generated with ffmpeg's test sources, so no media is committed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use muxmultitool_core::ffmpeg::{FfmpegCommand, Limits, Runner};
use muxmultitool_core::timeline::{
	AnalysisError, Cue, CueUpdate, Peak, ThumbnailUpdate, TimelineConfig, TimelineError,
	TimelineManager, WaveformUpdate,
};

const LIMITS: Duration = Duration::from_secs(120);

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The subtitles muxed into every clip.
const SUBS: &str = "WEBVTT\n\n00:00.500 --> 00:01.500\nHello <i>there</i>\n\n\
	00:02.000 --> 00:03.000\nSecond\nline two\n";

/// Writes `name` into `dir`: `secs` of test pattern (stream 0), a 440 Hz
/// tone at half scale for the first `tone_secs` seconds and silence after
/// (stream 1), and the two cues of [`SUBS`] as SubRip (stream 2).
async fn make_clip(
	runner: &Runner,
	dir: &Path,
	name: &str,
	secs: u32,
	tone_secs: u32,
) -> Result<PathBuf> {
	let subs = dir.join("subs.vtt");
	std::fs::write(&subs, SUBS)?;
	let clip = dir.join(name);
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input(format!("testsrc2=size=160x120:rate=25:duration={secs}"))
		.option("-f", "lavfi")
		.input(format!(
			"aevalsrc=if(lt(t\\,{tone_secs})\\,0.5*sin(440*2*PI*t)\\,0):s=48000:d={secs}"
		))
		.input(&subs)
		.option("-map", "0:v")
		.option("-map", "1:a")
		.option("-map", "2:s")
		.option("-c:v", "libx264")
		.option("-preset", "ultrafast")
		.option("-pix_fmt", "yuv420p")
		.option("-g", "50")
		.option("-c:a", "aac")
		.option("-c:s", "srt")
		.flag("-y")
		.output(&clip);
	runner.run(&cmd, &Limits::timeout(LIMITS), None).await?;
	Ok(clip)
}

/// Small sprites of 3 × 2 tiles, so a 4 s clip spans two of them.
fn config() -> TimelineConfig {
	TimelineConfig {
		tile_width: 80,
		columns: 3,
		rows: 2,
		..TimelineConfig::default()
	}
}

async fn thumbnail_updates(manager: &TimelineManager, clip: &Path) -> Result<Vec<ThumbnailUpdate>> {
	let mut watch = manager.thumbnails(clip).await?;
	let mut updates = Vec::new();
	while let Some(update) = watch.next().await {
		updates.push(update?);
	}
	Ok(updates)
}

async fn waveform_updates(
	manager: &TimelineManager,
	clip: &Path,
	stream: u32,
	from: f64,
	to: f64,
	count: usize,
) -> Result<Vec<WaveformUpdate>> {
	let mut watch = manager.waveform(clip, stream, from, to, count).await?;
	let mut updates = Vec::new();
	while let Some(update) = watch.next().await {
		updates.push(update?);
	}
	Ok(updates)
}

async fn cue_updates(
	manager: &TimelineManager,
	clip: &Path,
	stream: u32,
) -> Result<Vec<CueUpdate>> {
	let mut watch = manager.subtitle_cues(clip, stream).await?;
	let mut updates = Vec::new();
	while let Some(update) = watch.next().await {
		updates.push(update?);
	}
	Ok(updates)
}

/// The peaks of a window, assembled from its updates.
fn assemble(updates: &[WaveformUpdate]) -> Vec<Peak> {
	let mut peaks = Vec::new();
	for update in updates {
		assert_eq!(update.offset, peaks.len(), "updates are contiguous");
		peaks.extend_from_slice(&update.peaks);
	}
	peaks
}

#[tokio::test]
async fn thumbnails_are_generated_in_sprites_and_cached() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(&runner, dir.path(), "clip.mkv", 4, 2)
		.await
		.unwrap();
	let manager = TimelineManager::new(runner.clone(), config(), dir.path().join("timeline"));

	let updates = thumbnail_updates(&manager, &clip).await.unwrap();
	let first = &updates[0];
	let sheet = &first.sheet;
	assert!((sheet.interval_secs - 0.5).abs() < 1e-9, "{sheet:?}");
	assert_eq!((sheet.tile_width, sheet.tile_height), (80, 60));
	assert_eq!((sheet.columns, sheet.rows), (3, 2));
	// The container is a hair longer than 4 s (AAC padding), so the plan
	// may count a ninth tile; either way two sprites.
	assert!((8..=9).contains(&sheet.tile_count), "{sheet:?}");
	assert_eq!(sheet.sprite_count, 2);
	assert!(!sheet.keyframes_only);
	let last = updates.last().unwrap();
	assert!(last.done, "{updates:?}");
	assert_eq!(last.ready, 2, "{updates:?}");
	assert!((8..=9).contains(&last.sheet.tile_count), "{last:?}");
	assert_eq!(last.sheet.sprite_count, 2);
	assert!(
		updates.iter().all(|u| u.media_id == first.media_id),
		"{updates:?}"
	);
	assert!(
		updates.windows(2).all(|w| w[0].ready <= w[1].ready),
		"ready only grows: {updates:?}"
	);

	let id = &first.media_id;
	for n in 0..2 {
		let sprite = manager.sprite(id, n).expect("sprite is ready");
		let bytes = std::fs::read(&sprite).unwrap();
		assert_eq!(&bytes[..2], &[0xff, 0xd8], "sprite {n} is a JPEG");
		let probe = runner
			.probe(&sprite, &Limits::timeout(LIMITS))
			.await
			.unwrap();
		let picture = &probe.streams[0];
		assert_eq!((picture.width, picture.height), (Some(240), Some(120)));
	}
	assert!(manager.sprite(id, 2).is_none());
	assert!(manager.sprite("nope", 0).is_none());

	// Cached: one update, done, same id, nothing regenerated.
	let again = thumbnail_updates(&manager, &clip).await.unwrap();
	assert_eq!(again.len(), 1, "{again:?}");
	assert_eq!(again[0], *last);
}

#[tokio::test]
async fn waveform_peaks_follow_the_signal_at_any_resolution() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(&runner, dir.path(), "clip.mkv", 4, 2)
		.await
		.unwrap();
	let manager = TimelineManager::new(runner, config(), dir.path().join("timeline"));

	// The whole file in 8 peaks: tone for two seconds, then silence.
	let updates = waveform_updates(&manager, &clip, 1, 0.0, 0.0, 8)
		.await
		.unwrap();
	let window = updates[0].window;
	assert!(
		(window.from, window.to, window.count) == (0.0, 4.0, 8) || (window.to - 4.0).abs() < 0.1,
		"{window:?}"
	);
	assert!(updates.last().unwrap().done, "{updates:?}");
	assert!(updates.last().unwrap().analyzed_secs > 3.9, "{updates:?}");
	let peaks = assemble(&updates);
	assert_eq!(peaks.len(), 8);
	for (i, peak) in peaks.iter().enumerate() {
		if i < 4 {
			assert!(
				(0.4..=0.6).contains(&peak.max) && (-0.6..=-0.4).contains(&peak.min),
				"peak {i} is the tone: {peak:?}"
			);
		} else {
			assert!(
				peak.max.abs() < 0.05 && peak.min.abs() < 0.05,
				"peak {i} is silence: {peak:?}"
			);
		}
	}

	// A window over the transition, from the cache: one update.
	let updates = waveform_updates(&manager, &clip, 1, 1.0, 3.0, 4)
		.await
		.unwrap();
	assert_eq!(updates.len(), 1, "{updates:?}");
	let peaks = assemble(&updates);
	assert!(peaks[0].max > 0.4 && peaks[1].max > 0.4, "{peaks:?}");
	assert!(peaks[2].max < 0.05 && peaks[3].max < 0.05, "{peaks:?}");

	// Finer than the base resolution still answers.
	let updates = waveform_updates(&manager, &clip, 1, 0.0, 0.01, 10)
		.await
		.unwrap();
	assert_eq!(assemble(&updates).len(), 10);

	// Bad windows and streams.
	assert!(matches!(
		manager.waveform(&clip, 1, 0.0, 0.0, 0).await,
		Err(TimelineError::InvalidWindow(_))
	));
	assert!(matches!(
		manager.waveform(&clip, 1, 0.0, 0.0, 9000).await,
		Err(TimelineError::InvalidWindow(_))
	));
	assert!(matches!(
		manager.waveform(&clip, 1, 5.0, 0.0, 10).await,
		Err(TimelineError::InvalidWindow(_))
	));
	assert!(matches!(
		manager.waveform(&clip, 1, 2.0, 1.0, 10).await,
		Err(TimelineError::InvalidWindow(_))
	));
	assert!(matches!(
		manager.waveform(&clip, 0, 0.0, 0.0, 10).await,
		Err(TimelineError::NotAudio(0))
	));
	assert!(matches!(
		manager.waveform(&clip, 9, 0.0, 0.0, 10).await,
		Err(TimelineError::NoStream(9))
	));
}

#[tokio::test]
async fn subtitle_cues_are_read_as_plain_text() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(&runner, dir.path(), "clip.mkv", 4, 2)
		.await
		.unwrap();
	let manager = TimelineManager::new(runner, config(), dir.path().join("timeline"));

	let updates = cue_updates(&manager, &clip, 2).await.unwrap();
	assert!(updates.last().unwrap().done, "{updates:?}");
	let cues: Vec<Cue> = updates.into_iter().flat_map(|u| u.cues).collect();
	assert_eq!(
		cues,
		[
			Cue {
				start: 0.5,
				end: 1.5,
				text: String::from("Hello there")
			},
			Cue {
				start: 2.0,
				end: 3.0,
				text: String::from("Second\nline two")
			}
		]
	);

	// Cached: one update with everything.
	let again = cue_updates(&manager, &clip, 2).await.unwrap();
	assert_eq!(again.len(), 1, "{again:?}");
	assert_eq!(again[0].cues.len(), 2);
	assert!(again[0].done);

	assert!(matches!(
		manager.subtitle_cues(&clip, 1).await,
		Err(TimelineError::NotSubtitle(1))
	));
	assert!(matches!(
		manager.subtitle_cues(&clip, 7).await,
		Err(TimelineError::NoStream(7))
	));
}

#[tokio::test]
async fn a_changed_file_gets_a_new_id_and_fresh_results() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(&runner, dir.path(), "clip.mkv", 4, 2)
		.await
		.unwrap();
	let manager = TimelineManager::new(runner.clone(), config(), dir.path().join("timeline"));

	let before = thumbnail_updates(&manager, &clip).await.unwrap();
	let old_id = before[0].media_id.clone();
	let old_sprite = manager.sprite(&old_id, 0).unwrap();
	assert!(old_sprite.is_file());
	let cues: Vec<Cue> = cue_updates(&manager, &clip, 2)
		.await
		.unwrap()
		.into_iter()
		.flat_map(|u| u.cues)
		.collect();
	assert_eq!(cues.len(), 2);

	// The file is replaced by a longer one: a new size, so a new fingerprint.
	make_clip(&runner, dir.path(), "clip.mkv", 6, 3)
		.await
		.unwrap();
	let after = thumbnail_updates(&manager, &clip).await.unwrap();
	let new_id = &after[0].media_id;
	assert_ne!(*new_id, old_id);
	let tiles = after.last().unwrap().sheet.tile_count;
	assert!((12..=13).contains(&tiles), "{after:?}");
	assert!(manager.sprite(&old_id, 0).is_none(), "the old id is gone");
	assert!(!old_sprite.exists(), "the old sprites are removed");
	assert!(manager.sprite(new_id, 1).is_some());

	// The waveform is re-analysed as well: the tone now lasts three seconds.
	let peaks = assemble(
		&waveform_updates(&manager, &clip, 1, 0.0, 0.0, 6)
			.await
			.unwrap(),
	);
	assert!(peaks[2].max > 0.4 && peaks[3].max < 0.05, "{peaks:?}");
}

#[tokio::test]
async fn the_cache_is_bounded_and_can_be_emptied() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let a = make_clip(&runner, dir.path(), "a.mkv", 4, 2).await.unwrap();
	let b = make_clip(&runner, dir.path(), "b.mkv", 4, 2).await.unwrap();
	let manager = TimelineManager::new(
		runner,
		TimelineConfig {
			max_files: 1,
			..config()
		},
		dir.path().join("timeline"),
	);

	let id_a = thumbnail_updates(&manager, &a).await.unwrap()[0]
		.media_id
		.clone();
	assert!(manager.sprite(&id_a, 0).is_some());
	let id_b = thumbnail_updates(&manager, &b).await.unwrap()[0]
		.media_id
		.clone();
	assert!(manager.sprite(&id_a, 0).is_none(), "a was evicted");
	assert!(manager.sprite(&id_b, 0).is_some());

	assert!(manager.forget(&b).await);
	assert!(!manager.forget(&b).await);
	assert!(manager.sprite(&id_b, 0).is_none());

	let id_b2 = thumbnail_updates(&manager, &b).await.unwrap()[0]
		.media_id
		.clone();
	assert_ne!(id_b2, id_b);
	manager.shutdown().await;
	assert!(!dir.path().join("timeline").exists());
	assert!(manager.sprite(&id_b2, 0).is_none());
}

#[tokio::test]
async fn dropping_a_file_cancels_its_running_analysis() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(&runner, dir.path(), "clip.mkv", 60, 30)
		.await
		.unwrap();
	let manager = TimelineManager::new(runner, config(), dir.path().join("timeline"));

	let mut watch = manager.thumbnails(&clip).await.unwrap();
	let first = watch.next().await.unwrap().unwrap();
	assert!(!first.done);
	assert!(manager.forget(&clip).await);
	let mut outcome = watch.next().await;
	// A sprite may have become ready before the cancel landed.
	while let Some(Ok(update)) = &outcome {
		assert!(!update.done, "{update:?}");
		outcome = watch.next().await;
	}
	assert_eq!(outcome, Some(Err(AnalysisError::Cancelled)));
	assert!(watch.next().await.is_none());
	assert!(manager.sprite(&first.media_id, 0).is_none());
}

#[tokio::test]
async fn unreadable_sources_are_errors() {
	let dir = tempfile::tempdir().unwrap();
	let manager = TimelineManager::new(Runner::default(), config(), dir.path().join("timeline"));
	assert!(matches!(
		manager.thumbnails(&dir.path().join("missing.mkv")).await,
		Err(TimelineError::Open { .. })
	));
	assert!(matches!(
		manager.thumbnails(dir.path()).await,
		Err(TimelineError::NotAFile(_))
	));
	let text = dir.path().join("notes.txt");
	std::fs::write(&text, "not media").unwrap();
	assert!(matches!(
		manager.waveform(&text, 0, 0.0, 0.0, 10).await,
		Err(TimelineError::Probe(_))
	));
	// An audio-only file has no thumbnails.
	let audio = dir.path().join("tone.mka");
	let cmd = FfmpegCommand::new()
		.option("-f", "lavfi")
		.input("sine=frequency=440:sample_rate=48000:duration=1")
		.option("-c:a", "aac")
		.flag("-y")
		.output(&audio);
	Runner::default()
		.run(&cmd, &Limits::timeout(LIMITS), None)
		.await
		.unwrap();
	assert!(matches!(
		manager.thumbnails(&audio).await,
		Err(TimelineError::NoVideo)
	));
	// But a waveform.
	let peaks = assemble(
		&waveform_updates(&manager, &audio, 0, 0.0, 0.0, 2)
			.await
			.unwrap(),
	);
	assert_eq!(peaks.len(), 2);
	assert!(peaks[0].max > 0.05, "{peaks:?}");
}

#[tokio::test]
async fn a_temp_dir_manager_uses_a_random_folder_and_random_ids() {
	let dir = tempfile::tempdir().unwrap();
	let runner = Runner::default();
	let clip = make_clip(&runner, dir.path(), "clip.mkv", 2, 2)
		.await
		.unwrap();
	let manager = TimelineManager::in_temp_dir(runner.clone(), config());
	let other = TimelineManager::in_temp_dir(runner, config());

	let updates = thumbnail_updates(&manager, &clip).await.unwrap();
	let id = updates[0].media_id.clone();
	assert_eq!(id.len(), 32, "{id}");
	assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
	let again = thumbnail_updates(&other, &clip).await.unwrap();
	let other_id = again[0].media_id.clone();
	assert_ne!(id, other_id, "ids are random, not a counter");

	// base/<id>/thumbs/0.jpg
	let base = manager
		.sprite(&id, 0)
		.unwrap()
		.ancestors()
		.nth(3)
		.unwrap()
		.to_path_buf();
	let name = base.file_name().unwrap().to_string_lossy().into_owned();
	assert!(base.starts_with(std::env::temp_dir()), "{}", base.display());
	assert!(name.starts_with("muxmultitool-timeline-"), "{name}");
	assert_ne!(
		name,
		format!("muxmultitool-timeline-{}", std::process::id())
	);
	let other_base = other
		.sprite(&other_id, 0)
		.unwrap()
		.ancestors()
		.nth(3)
		.unwrap()
		.to_path_buf();
	assert_ne!(base, other_base, "every manager has a folder of its own");

	manager.shutdown().await;
	assert!(!base.exists(), "shutdown removes the folder");
	drop(other);
	assert!(!other_base.exists(), "a dropped manager removes its folder");
}
