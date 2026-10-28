//! Combining streams from several source files into one output, without
//! re-encoding: English subtitles from one MKV into another, or one MKV
//! from a BD rip's video and a Blu-ray's audio and subtitles (gui/03-combine).
//!
//! The user lists [`Source`]s (A, B, C, …) and the [`Track`]s of the output
//! in order, each naming a source and one of its streams. [`plan`] resolves
//! that into a [`Plan`]: one [`OutputTrack`] per track with the metadata
//! the output will carry, an [`Issue`] for each track the target
//! [`Container`] cannot hold (the table of [`crate::streams::containers`]),
//! and a [`Warning`] when the sources' durations differ noticeably. The
//! plan's [`command`](Plan::command) is one `-i` per used source, one
//! `-map` per track and `-c copy`, so what the user reviews is what ffmpeg
//! runs.
//!
//! Per-track metadata carries over from the source by default: ffmpeg
//! copies a stream's tags and disposition flags along with the stream. A
//! [`Track`] may set the language, the title and the default and forced
//! flags instead; only those edits become `-metadata` and `-disposition`
//! options. A source may be shifted in time (`-itsoffset`), which stays
//! lossless: the packets are copied, only their timestamps move.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cut::timecode;
use crate::ffmpeg::FfmpegCommand;
use crate::ffmpeg::probe::{CodecType, Disposition, ProbeResult, Stream};
use crate::streams::{CONTAINERS, Compatibility, Container};

pub mod command;

/// How much the durations of two sources may differ before the plan warns
/// about it. Sources that differ by more have likely been cut differently,
/// so their streams would drift apart or end at different times.
pub const DURATION_TOLERANCE: Duration = Duration::from_secs(1);

/// One input file, as the plan sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
	/// The file.
	pub path: PathBuf,
	/// The file name, as the messages call the source next to its label.
	pub name: String,
	/// The container's duration, when the probe knows it.
	pub duration: Option<Duration>,
	/// How far the source's timestamps are shifted, in seconds: positive
	/// delays it. Zero means no `-itsoffset`.
	pub offset_secs: f64,
	/// The streams in container order.
	pub streams: Vec<Stream>,
}

impl Source {
	/// The source at `path` as `probe` describes it, without an offset.
	#[must_use]
	pub fn from_probe(path: &Path, probe: &ProbeResult) -> Self {
		Self {
			path: path.to_path_buf(),
			name: path
				.file_name()
				.map(|n| n.to_string_lossy().into_owned())
				.unwrap_or_default(),
			duration: probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
			offset_secs: 0.0,
			streams: probe.streams.clone(),
		}
	}

	/// The same source, shifted by `secs`.
	#[must_use]
	pub fn with_offset(self, secs: f64) -> Self {
		Self {
			offset_secs: secs,
			..self
		}
	}

	/// The stream with `index`, if the source has one.
	#[must_use]
	pub fn stream(&self, index: u32) -> Option<&Stream> {
		self.streams.iter().find(|s| s.index == index)
	}
}

/// One track of the output: a stream of a source, with the metadata edits
/// the user made. A `None` keeps what the source has.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Track {
	/// The position of the source in the plan's list.
	pub source: usize,
	/// The stream's index in that source.
	pub stream: u32,
	/// The language tag to set; an empty string removes the tag.
	pub language: Option<String>,
	/// The title to set; an empty string removes it.
	pub title: Option<String>,
	/// Whether the track is the default of its kind.
	pub default: Option<bool>,
	/// Whether the track is forced (subtitles that are always shown).
	pub forced: Option<bool>,
}

impl Track {
	/// Stream `stream` of source `source`, with everything carried over.
	#[must_use]
	pub fn new(source: usize, stream: u32) -> Self {
		Self {
			source,
			stream,
			..Self::default()
		}
	}

	/// Sets the language.
	#[must_use]
	pub fn language(self, language: impl Into<String>) -> Self {
		Self {
			language: Some(language.into()),
			..self
		}
	}

	/// Sets the title.
	#[must_use]
	pub fn title(self, title: impl Into<String>) -> Self {
		Self {
			title: Some(title.into()),
			..self
		}
	}

	/// Sets the default flag.
	#[must_use]
	pub fn as_default(self, default: bool) -> Self {
		Self {
			default: Some(default),
			..self
		}
	}

	/// Sets the forced flag.
	#[must_use]
	pub fn as_forced(self, forced: bool) -> Self {
		Self {
			forced: Some(forced),
			..self
		}
	}
}

/// Why the tracks cannot be planned at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
	/// No source at all.
	#[error("no source file is given")]
	NoSources,
	/// No track at all.
	#[error("no track is added to the output")]
	NoTracks,
	/// A track names a source the plan does not have.
	#[error("track {track} names source {index}, but there are only {sources} sources")]
	UnknownSource {
		/// The track's position, from 1.
		track: usize,
		/// The source index the track names.
		index: usize,
		/// How many sources there are.
		sources: usize,
	},
	/// A track names a stream its source does not have.
	#[error("source {label} ({name}) has no stream {stream}")]
	UnknownStream {
		/// The source's label.
		label: String,
		/// The source's file name.
		name: String,
		/// The stream index the track names.
		stream: u32,
	},
	/// The same stream is added twice.
	#[error("{label}:#{stream} is added twice")]
	DuplicateTrack {
		/// The source's label.
		label: String,
		/// The stream's index.
		stream: u32,
	},
	/// A source's offset is not a number.
	#[error("the offset of source {label} is not a number")]
	BadOffset {
		/// The source's label.
		label: String,
	},
}

/// A source as the plan placed it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanSource {
	/// The position in the plan's list of sources.
	pub index: usize,
	/// The label the messages use: `A`, `B`, `C`, …
	pub label: String,
	/// The file name.
	pub name: String,
	/// The file.
	pub path: PathBuf,
	/// The source's position among the command's inputs (what `-map
	/// <input>:<stream>` refers to): the used sources numbered in source
	/// order. `None` when no track uses the source and the command leaves
	/// it out.
	pub input: Option<usize>,
	/// See [`Source::duration`].
	pub duration: Option<Duration>,
	/// See [`Source::offset_secs`].
	pub offset_secs: f64,
}

impl PlanSource {
	/// Whether at least one track comes from this source.
	#[must_use]
	pub const fn used(&self) -> bool {
		self.input.is_some()
	}
}

/// What the command sets on a track, beyond what carries over.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Edits {
	/// The `-metadata` tags to set, in order: `language`, `title`. An empty
	/// value removes the tag.
	pub metadata: Vec<(&'static str, String)>,
	/// The complete `-disposition` flag set to write, when a flag changes.
	pub disposition: Option<Vec<&'static str>>,
}

impl Edits {
	/// Whether the command sets anything.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.metadata.is_empty() && self.disposition.is_none()
	}
}

/// One stream of the output.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputTrack {
	/// The track's position in the output, from 0: what `-metadata:s:<n>`
	/// refers to.
	pub position: usize,
	/// The source's position in the plan's list.
	pub source: usize,
	/// The source's label.
	pub source_label: String,
	/// The source's position among the command's inputs.
	pub input: usize,
	/// The stream's index in the source.
	pub stream: u32,
	/// What the stream carries.
	pub kind: CodecType,
	/// The stream as the messages describe it, e.g. `hevc video`.
	pub description: String,
	/// The language the output will carry.
	pub language: Option<String>,
	/// The title the output will carry.
	pub title: Option<String>,
	/// Whether the output track is the default of its kind.
	pub default: bool,
	/// Whether the output track is forced.
	pub forced: bool,
	/// What the command sets, beyond what carries over.
	pub edits: Edits,
}

impl OutputTrack {
	/// `B:#1`: the source's label and the stream's index, as the output
	/// table names the track's origin.
	#[must_use]
	pub fn origin(&self) -> String {
		format!("{}:#{}", self.source_label, self.stream)
	}
}

/// A track the container cannot hold, or may not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
	/// The track's position, from 0.
	pub track: usize,
	/// True when ffmpeg is known to refuse the combination, so the job
	/// would fail at once; false for a warning about an uncertain one.
	pub blocking: bool,
	/// The problem and what to do, for the user.
	pub message: String,
}

impl fmt::Display for Issue {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.message)
	}
}

/// Something to check before running, which does not block the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
	/// Two used sources with video or audio last noticeably different
	/// times; see [`DURATION_TOLERANCE`].
	DurationsDiffer {
		/// The reference: the first used source with video or audio, with
		/// its duration.
		reference: (String, Duration),
		/// The source that differs, with its duration.
		other: (String, Duration),
	},
}

impl fmt::Display for Warning {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::DurationsDiffer { reference, other } => {
				let secs = other.1.as_secs_f64() - reference.1.as_secs_f64();
				let sign = if secs < 0.0 { "-" } else { "+" };
				write!(
					f,
					"durations differ: {} {}, {} {} ({sign}{:.1} s)",
					reference.0,
					timecode::format(reference.1),
					other.0,
					timecode::format(other.1),
					secs.abs()
				)
			}
		}
	}
}

/// Tracks resolved against their sources: what the output will hold.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
	/// The target container.
	pub container: Container,
	/// Every source, in the order given.
	pub sources: Vec<PlanSource>,
	/// The output's streams, in order.
	pub tracks: Vec<OutputTrack>,
	/// The tracks the container cannot or may not hold, in track order.
	pub issues: Vec<Issue>,
	/// What to check before running.
	pub warnings: Vec<Warning>,
}

impl Plan {
	/// Whether no blocking issue stands in the way of running the job.
	#[must_use]
	pub fn can_run(&self) -> bool {
		!self.issues.iter().any(|i| i.blocking)
	}

	/// The blocking issues' messages, for a refusal.
	#[must_use]
	pub fn blocking_messages(&self) -> Vec<String> {
		self.issues
			.iter()
			.filter(|i| i.blocking)
			.map(|i| i.message.clone())
			.collect()
	}

	/// The sources the command reads, in input order.
	pub fn inputs(&self) -> impl Iterator<Item = &PlanSource> + '_ {
		self.sources.iter().filter(|s| s.used())
	}

	/// How long the output should last: the longest used source with video
	/// or audio, its offset included. `None` when no such duration is known.
	#[must_use]
	pub fn expected_duration(&self) -> Option<Duration> {
		self.timed_sources()
			.filter_map(|source| {
				let secs = source.duration?.as_secs_f64() + source.offset_secs.max(0.0);
				Duration::try_from_secs_f64(secs).ok()
			})
			.max()
	}

	/// The ffmpeg command that writes the tracks into `output`; see
	/// [`command::combine`].
	#[must_use]
	pub fn command(&self, output: &Path) -> FfmpegCommand {
		command::combine(self, output)
	}

	/// The used sources that contribute video or audio: the ones whose
	/// durations are compared. A subtitle file's duration is only its last
	/// cue, so subtitle-only sources stay out of it.
	fn timed_sources(&self) -> impl Iterator<Item = &PlanSource> + '_ {
		self.sources.iter().filter(|source| {
			self.tracks.iter().any(|t| {
				t.source == source.index && matches!(t.kind, CodecType::Video | CodecType::Audio)
			})
		})
	}
}

/// Resolves `tracks` against `sources` for `container`.
///
/// # Errors
/// See [`PlanError`]: nothing to combine, a track that names a source or
/// stream that does not exist, a stream added twice, or an offset that is
/// not a number. A track the container cannot hold is not an error here
/// but an [`Issue`] on the plan.
pub fn plan(sources: &[Source], tracks: &[Track], container: Container) -> Result<Plan, PlanError> {
	if sources.is_empty() {
		return Err(PlanError::NoSources);
	}
	if tracks.is_empty() {
		return Err(PlanError::NoTracks);
	}
	for (i, source) in sources.iter().enumerate() {
		if !source.offset_secs.is_finite() {
			return Err(PlanError::BadOffset { label: label(i) });
		}
	}
	let mut used = vec![false; sources.len()];
	let mut streams: Vec<&Stream> = Vec::with_capacity(tracks.len());
	for (position, track) in tracks.iter().enumerate() {
		let Some(source) = sources.get(track.source) else {
			return Err(PlanError::UnknownSource {
				track: position + 1,
				index: track.source,
				sources: sources.len(),
			});
		};
		let Some(stream) = source.stream(track.stream) else {
			return Err(PlanError::UnknownStream {
				label: label(track.source),
				name: source.name.clone(),
				stream: track.stream,
			});
		};
		streams.push(stream);
		if tracks[..position]
			.iter()
			.any(|t| t.source == track.source && t.stream == track.stream)
		{
			return Err(PlanError::DuplicateTrack {
				label: label(track.source),
				stream: track.stream,
			});
		}
		used[track.source] = true;
	}
	// The used sources are the inputs, in source order.
	let mut next_input = 0;
	let inputs: Vec<Option<usize>> = used
		.iter()
		.map(|used| {
			used.then(|| {
				next_input += 1;
				next_input - 1
			})
		})
		.collect();
	let plan_sources: Vec<PlanSource> = sources
		.iter()
		.zip(&inputs)
		.enumerate()
		.map(|(index, (source, input))| PlanSource {
			index,
			label: label(index),
			name: source.name.clone(),
			path: source.path.clone(),
			input: *input,
			duration: source.duration,
			offset_secs: source.offset_secs,
		})
		.collect();
	let output_tracks: Vec<OutputTrack> = tracks
		.iter()
		.zip(&streams)
		.enumerate()
		.map(|(position, (track, stream))| {
			resolve(position, track, stream, &plan_sources[track.source])
		})
		.collect();
	let issues = output_tracks
		.iter()
		.zip(&streams)
		.filter_map(|(track, stream)| issue(track, stream, container))
		.collect();
	let mut plan = Plan {
		container,
		sources: plan_sources,
		tracks: output_tracks,
		issues,
		warnings: Vec::new(),
	};
	plan.warnings = duration_warnings(&plan);
	Ok(plan)
}

/// The output track for `track`, which copies `stream`.
fn resolve(position: usize, track: &Track, stream: &Stream, source: &PlanSource) -> OutputTrack {
	let source_language = stream.language().map(str::to_owned);
	let source_title = stream
		.tags
		.get("title")
		.or_else(|| stream.tags.get("name"))
		.cloned();
	let mut edits = Edits::default();
	let language = match &track.language {
		Some(set) if set != &source_language.clone().unwrap_or_default() => {
			edits.metadata.push(("language", set.clone()));
			(!set.is_empty()).then(|| set.clone())
		}
		_ => source_language,
	};
	let title = match &track.title {
		Some(set) if set != &source_title.clone().unwrap_or_default() => {
			edits.metadata.push(("title", set.clone()));
			(!set.is_empty()).then(|| set.clone())
		}
		_ => source_title,
	};
	let flags = Disposition {
		default: track.default.unwrap_or(stream.disposition.default),
		forced: track.forced.unwrap_or(stream.disposition.forced),
		..stream.disposition
	};
	if flags != stream.disposition {
		edits.disposition = Some(flag_names(flags));
	}
	OutputTrack {
		position,
		source: source.index,
		source_label: source.label.clone(),
		input: source.input.unwrap_or_default(),
		stream: stream.index,
		kind: stream.codec_type,
		description: Container::describe(stream),
		language,
		title,
		default: flags.default,
		forced: flags.forced,
		edits,
	}
}

/// The names of the set flags, as `-disposition` takes them.
fn flag_names(d: Disposition) -> Vec<&'static str> {
	[
		("default", d.default),
		("dub", d.dub),
		("original", d.original),
		("comment", d.comment),
		("forced", d.forced),
		("hearing_impaired", d.hearing_impaired),
		("visual_impaired", d.visual_impaired),
		("attached_pic", d.attached_pic),
		("captions", d.captions),
		("descriptions", d.descriptions),
	]
	.into_iter()
	.filter(|&(_, set)| set)
	.map(|(name, _)| name)
	.collect()
}

/// The issue of keeping `track` in `container`, if any.
fn issue(track: &OutputTrack, stream: &Stream, container: Container) -> Option<Issue> {
	let what = &track.description;
	let number = track.position + 1;
	let origin = track.origin();
	match container.holds(stream) {
		Compatibility::Supported => None,
		Compatibility::Unsupported => {
			let others: Vec<&str> = CONTAINERS
				.into_iter()
				.filter(|c| *c != container && c.holds(stream) == Compatibility::Supported)
				.map(Container::name)
				.collect();
			let advice = if others.is_empty() {
				"remove the track".to_owned()
			} else {
				format!("remove the track or choose {}", others.join(" or "))
			};
			Some(Issue {
				track: track.position,
				blocking: true,
				message: format!(
					"{container} cannot hold {what} (track {number}, {origin}); {advice}"
				),
			})
		}
		Compatibility::Uncertain => Some(Issue {
			track: track.position,
			blocking: false,
			message: format!(
				"{container} may not hold {what} (track {number}, {origin}); ffmpeg decides when the job runs"
			),
		}),
	}
}

/// A warning for every used source with video or audio whose duration is
/// further than [`DURATION_TOLERANCE`] from the first such source's.
fn duration_warnings(plan: &Plan) -> Vec<Warning> {
	let mut timed = plan.timed_sources().filter_map(|s| Some((s, s.duration?)));
	let Some((reference, reference_duration)) = timed.next() else {
		return Vec::new();
	};
	timed
		.filter(|(_, duration)| duration.abs_diff(reference_duration) > DURATION_TOLERANCE)
		.map(|(other, duration)| Warning::DurationsDiffer {
			reference: (describe(reference), reference_duration),
			other: (describe(other), duration),
		})
		.collect()
}

/// `A (Movie.mkv)`, as the warnings name a source.
fn describe(source: &PlanSource) -> String {
	format!("{} ({})", source.label, source.name)
}

/// The label of the source at `index`: `A` … `Z`, then `AA`, `AB`, …
#[must_use]
pub fn label(index: usize) -> String {
	let mut letters = Vec::new();
	let mut n = index;
	loop {
		letters.push(char::from(b'A' + u8::try_from(n % 26).unwrap_or_default()));
		n /= 26;
		if n == 0 {
			break;
		}
		n -= 1;
	}
	letters.iter().rev().collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe::parse;

	/// A: the BD rip, HEVC video and stereo AAC, 2:01:33.
	fn bdrip() -> Source {
		let probe = parse(
			r#"{"format":{"duration":"7293.0"},"streams":[
				{"index":0,"codec_type":"video","codec_name":"hevc","disposition":{"default":1}},
				{"index":1,"codec_type":"audio","codec_name":"aac","tags":{"language":"eng"},"disposition":{"default":1}}
			]}"#,
		)
		.unwrap();
		Source::from_probe(Path::new("/media/in/Movie.2024.BDRip.mkv"), &probe)
	}

	/// B: the Blu-ray, with `TrueHD`, two AC-3 tracks and PGS subtitles,
	/// 2:01:35.
	fn bluray() -> Source {
		let probe = parse(
			r#"{"format":{"duration":"7295.0"},"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264"},
				{"index":1,"codec_type":"audio","codec_name":"truehd","tags":{"language":"eng","title":"Atmos"},"disposition":{"default":1}},
				{"index":2,"codec_type":"audio","codec_name":"ac3","tags":{"language":"eng"}},
				{"index":3,"codec_type":"audio","codec_name":"ac3","tags":{"language":"fre"}},
				{"index":6,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle","tags":{"language":"eng"}},
				{"index":7,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle","tags":{"language":"fre"},"disposition":{"forced":1}}
			]}"#,
		)
		.unwrap();
		Source::from_probe(Path::new("/media/in/Movie.2024.BluRay.m2ts"), &probe)
	}

	/// C: a SubRip file, whose "duration" is its last cue.
	fn srt() -> Source {
		let probe = parse(
			r#"{"format":{"duration":"7100.5"},"streams":[
				{"index":0,"codec_type":"subtitle","codec_name":"subrip"}
			]}"#,
		)
		.unwrap();
		Source::from_probe(Path::new("/media/in/Movie.2024.eng.srt"), &probe)
	}

	fn sources() -> Vec<Source> {
		vec![bdrip(), bluray(), srt()]
	}

	fn origins(plan: &Plan) -> Vec<String> {
		plan.tracks.iter().map(OutputTrack::origin).collect()
	}

	#[test]
	fn tracks_keep_their_order_and_the_sources_their_metadata() {
		let tracks = [
			Track::new(0, 0),
			Track::new(1, 1),
			Track::new(1, 2),
			Track::new(2, 0),
		];
		let plan = plan(&sources(), &tracks, Container::Mkv).unwrap();
		assert_eq!(origins(&plan), ["A:#0", "B:#1", "B:#2", "C:#0"]);
		let inputs: Vec<usize> = plan.tracks.iter().map(|t| t.input).collect();
		assert_eq!(inputs, [0, 1, 1, 2]);
		assert!(plan.sources.iter().all(PlanSource::used));
		assert_eq!(plan.tracks[1].language.as_deref(), Some("eng"));
		assert_eq!(plan.tracks[1].title.as_deref(), Some("Atmos"));
		assert!(plan.tracks[1].default);
		assert!(!plan.tracks[1].forced);
		assert!(plan.tracks[1].edits.is_empty());
		assert_eq!(plan.tracks[1].description, "truehd audio");
		assert_eq!(plan.tracks[3].language, None);
		assert_eq!(plan.tracks[3].kind, CodecType::Subtitle);
		assert_eq!(plan.issues, Vec::<Issue>::new());
		assert!(plan.can_run());
	}

	#[test]
	fn edits_become_metadata_and_disposition_options() {
		let tracks = [
			Track::new(0, 0),
			Track::new(1, 1).title("Atmos").as_default(true),
			Track::new(1, 2).title("Compatibility").as_default(false),
			Track::new(2, 0)
				.language("eng")
				.title("English")
				.as_forced(true),
		];
		let plan = plan(&sources(), &tracks, Container::Mkv).unwrap();
		// The same title and flag as the source: nothing to set.
		assert!(plan.tracks[1].edits.is_empty());
		assert_eq!(
			plan.tracks[2].edits,
			Edits {
				metadata: vec![("title", "Compatibility".into())],
				disposition: None,
			}
		);
		assert_eq!(plan.tracks[2].title.as_deref(), Some("Compatibility"));
		assert_eq!(
			plan.tracks[3].edits,
			Edits {
				metadata: vec![("language", "eng".into()), ("title", "English".into())],
				disposition: Some(vec!["forced"]),
			}
		);
		assert_eq!(plan.tracks[3].language.as_deref(), Some("eng"));
		assert!(plan.tracks[3].forced);
		assert!(!plan.tracks[3].default);
	}

	#[test]
	fn an_empty_value_removes_a_tag_and_a_flag_can_be_cleared() {
		let tracks = [Track::new(1, 7).title("").language("").as_forced(false)];
		let plan = plan(&sources(), &tracks, Container::Mkv).unwrap();
		let track = &plan.tracks[0];
		assert_eq!(track.language, None);
		assert_eq!(track.title, None);
		assert!(!track.forced);
		assert_eq!(
			track.edits,
			Edits {
				metadata: vec![("language", String::new())],
				disposition: Some(vec![]),
			}
		);
		// Only the track's source is an input.
		assert_eq!(track.input, 0);
		let used: Vec<&str> = plan.inputs().map(|s| s.label.as_str()).collect();
		assert_eq!(used, ["B"]);
		assert_eq!(plan.sources[0].input, None);
	}

	#[test]
	fn the_container_reports_what_it_cannot_hold() {
		let tracks = [Track::new(0, 0), Track::new(1, 1), Track::new(1, 6)];
		let plan = plan(&sources(), &tracks, Container::Mp4).unwrap();
		assert_eq!(plan.issues.len(), 2);
		assert!(!plan.can_run());
		assert_eq!(
			plan.blocking_messages(),
			[
				"MP4 cannot hold truehd audio (track 2, B:#1); remove the track or choose MKV",
				"MP4 cannot hold hdmv_pgs_subtitle subtitles (track 3, B:#6); remove the track or choose MKV",
			]
		);
		assert_eq!(plan.issues[0].track, 1);
		assert_eq!(plan.issues[0].to_string(), plan.issues[0].message);

		let odd =
			parse(r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"cfhd"}]}"#).unwrap();
		let odd = Source::from_probe(Path::new("/media/in/odd.mov"), &odd);
		let plan = super::plan(&[odd], &[Track::new(0, 0)], Container::Mp4).unwrap();
		assert!(plan.can_run());
		assert_eq!(
			plan.issues[0].message,
			"MP4 may not hold cfhd video (track 1, A:#0); ffmpeg decides when the job runs"
		);
	}

	#[test]
	fn durations_that_differ_are_a_warning_but_not_for_subtitle_files() {
		let tracks = [Track::new(0, 0), Track::new(1, 1), Track::new(2, 0)];
		let plan = plan(&sources(), &tracks, Container::Mkv).unwrap();
		assert_eq!(plan.warnings.len(), 1);
		assert_eq!(
			plan.warnings[0].to_string(),
			"durations differ: A (Movie.2024.BDRip.mkv) 2:01:33.000, B (Movie.2024.BluRay.m2ts) 2:01:35.000 (+2.0 s)"
		);
		assert_eq!(plan.expected_duration(), Some(Duration::from_secs(7295)));

		// Subtitles from B only: B's duration is not compared.
		let plan = super::plan(
			&sources(),
			&[Track::new(0, 0), Track::new(1, 6)],
			Container::Mkv,
		)
		.unwrap();
		assert_eq!(plan.warnings, Vec::<Warning>::new());
		assert_eq!(plan.expected_duration(), Some(Duration::from_secs(7293)));

		// Within the tolerance: no warning. The other way round: a minus.
		let mut shorter = bluray();
		shorter.duration = Some(Duration::from_secs_f64(7292.5));
		let plan = super::plan(&[bdrip(), shorter], &tracks[..2], Container::Mkv).unwrap();
		assert_eq!(plan.warnings, Vec::<Warning>::new());
		let mut shorter = bluray();
		shorter.duration = Some(Duration::from_secs(7200));
		let plan = super::plan(&[bdrip(), shorter], &tracks[..2], Container::Mkv).unwrap();
		assert!(plan.warnings[0].to_string().ends_with("(-93.0 s)"));
		assert_eq!(
			plan.warnings[0],
			Warning::DurationsDiffer {
				reference: ("A (Movie.2024.BDRip.mkv)".into(), Duration::from_secs(7293)),
				other: (
					"B (Movie.2024.BluRay.m2ts)".into(),
					Duration::from_secs(7200)
				),
			}
		);
	}

	#[test]
	fn an_offset_delays_the_source_and_counts_for_the_duration() {
		let sources = [bdrip(), bluray().with_offset(2.5)];
		let tracks = [Track::new(1, 0), Track::new(0, 1)];
		let plan = plan(&sources, &tracks, Container::Mkv).unwrap();
		assert_eq!(plan.sources[1].offset_secs, 2.5);
		// The inputs follow the source order, not the track order.
		assert_eq!(plan.tracks[0].input, 1);
		assert_eq!(plan.tracks[1].input, 0);
		assert_eq!(
			plan.expected_duration(),
			Some(Duration::from_secs_f64(7297.5))
		);
	}

	#[test]
	fn impossible_plans_are_errors() {
		let bad = |sources: &[Source], tracks: &[Track]| {
			plan(sources, tracks, Container::Mkv).unwrap_err()
		};
		assert_eq!(bad(&[], &[Track::new(0, 0)]), PlanError::NoSources);
		assert_eq!(bad(&sources(), &[]), PlanError::NoTracks);
		assert_eq!(
			bad(&sources(), &[Track::new(0, 0), Track::new(3, 0)]),
			PlanError::UnknownSource {
				track: 2,
				index: 3,
				sources: 3
			}
		);
		assert_eq!(
			bad(&sources(), &[Track::new(1, 4)]),
			PlanError::UnknownStream {
				label: "B".into(),
				name: "Movie.2024.BluRay.m2ts".into(),
				stream: 4
			}
		);
		assert_eq!(
			bad(
				&sources(),
				&[Track::new(1, 1), Track::new(0, 0), Track::new(1, 1)]
			),
			PlanError::DuplicateTrack {
				label: "B".into(),
				stream: 1
			}
		);
		assert_eq!(
			bad(&[bdrip().with_offset(f64::NAN)], &[Track::new(0, 0)]),
			PlanError::BadOffset { label: "A".into() }
		);
		assert_eq!(
			PlanError::UnknownStream {
				label: "B".into(),
				name: "b.mkv".into(),
				stream: 4
			}
			.to_string(),
			"source B (b.mkv) has no stream 4"
		);
		assert_eq!(
			PlanError::UnknownSource {
				track: 2,
				index: 3,
				sources: 3
			}
			.to_string(),
			"track 2 names source 3, but there are only 3 sources"
		);
	}

	#[test]
	fn labels_run_through_the_alphabet_and_on() {
		assert_eq!(label(0), "A");
		assert_eq!(label(25), "Z");
		assert_eq!(label(26), "AA");
		assert_eq!(label(27), "AB");
		assert_eq!(label(54), "BC");
		assert_eq!(label(702), "AAA");
	}

	#[test]
	fn undetermined_counts_as_a_language_the_source_has() {
		let mp4 = parse(
			r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"aac","tags":{"language":"und"}}]}"#,
		)
		.unwrap();
		let source = Source::from_probe(Path::new("/media/in/clip.mp4"), &mp4);
		let plan = plan(
			&[source],
			&[Track::new(0, 0).language("eng")],
			Container::Mkv,
		)
		.unwrap();
		assert_eq!(plan.tracks[0].language.as_deref(), Some("eng"));
		assert_eq!(
			plan.tracks[0].edits.metadata,
			[("language", "eng".to_owned())]
		);
	}
}
