//! Remux and selective stream copy: which streams of a file go into a new
//! container, without re-encoding.
//!
//! A [`Selection`] names the streams to keep in one of four ways: all of
//! them (a remux), a ticked set, everything but an unticked set, or the
//! video only. [`plan`] resolves it against the file's streams into a
//! [`Plan`]: every stream marked kept or dropped with the [`Reason`], plus
//! an [`Issue`] for each kept stream the target [`Container`] cannot hold
//! (see [`containers`]). The plan's [`command`](Plan::command) is a plain
//! `-map` + `-c copy` line, so what the user reviews is what ffmpeg runs.
//! A job is only submitted for a plan without blocking issues, so a
//! combination the container refuses is reported before anything runs.
//!
//! Every later stream operation (rule-based filters, combining sources,
//! extraction, batch) builds on the same plan.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::ffmpeg::FfmpegCommand;
use crate::ffmpeg::probe::{CodecType, ProbeResult, Stream};

pub mod command;
pub mod containers;

pub use containers::{CONTAINERS, Compatibility, Container};

/// How a [`Selection`] names the streams it keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
	/// Every stream is copied: a remux.
	#[default]
	All,
	/// Only the listed streams are copied.
	Keep,
	/// Every stream but the listed ones is copied.
	Drop,
	/// Only the video streams are copied. Cover art (a picture stored as a
	/// video stream with the `attached_pic` flag) is not video.
	VideoOnly,
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
	/// See [`Mode`].
	pub mode: Mode,
	/// The stream indexes [`Mode::Keep`] keeps or [`Mode::Drop`] drops. The
	/// other modes ignore them.
	pub streams: BTreeSet<u32>,
}

impl Selection {
	/// Every stream.
	#[must_use]
	pub fn all() -> Self {
		Self::default()
	}

	/// Only `streams`.
	pub fn keep(streams: impl IntoIterator<Item = u32>) -> Self {
		Self {
			mode: Mode::Keep,
			streams: streams.into_iter().collect(),
		}
	}

	/// Everything but `streams`.
	pub fn drop(streams: impl IntoIterator<Item = u32>) -> Self {
		Self {
			mode: Mode::Drop,
			streams: streams.into_iter().collect(),
		}
	}

	/// The video streams only.
	#[must_use]
	pub fn video_only() -> Self {
		Self {
			mode: Mode::VideoOnly,
			streams: BTreeSet::new(),
		}
	}
}

/// Why a stream is kept or dropped: the report's "why" column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
	/// Kept: every stream is copied.
	All,
	/// Kept: ticked by hand.
	Ticked,
	/// Dropped: unticked by hand.
	Unticked,
	/// Kept: it is video and the output is video only.
	Video,
	/// Dropped: not video.
	NotVideo,
	/// Dropped: cover art, a picture stored as a video stream.
	CoverArt,
}

impl fmt::Display for Reason {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::All => "all streams",
			Self::Ticked => "ticked",
			Self::Unticked => "unticked",
			Self::Video => "video",
			Self::NotVideo => "not video",
			Self::CoverArt => "cover art, not video",
		})
	}
}

/// What happens to one stream of the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
	/// The stream's index, what `-map 0:<index>` refers to.
	pub index: u32,
	/// Whether it is copied to the output.
	pub kept: bool,
	/// Why.
	pub reason: Reason,
}

/// A kept stream the container cannot hold, or may not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
	/// The stream's index.
	pub index: u32,
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

/// Why a selection cannot be planned at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
	/// The selection names a stream the input does not have.
	#[error("the input has no stream {0}")]
	UnknownStream(u32),
	/// Nothing would be copied.
	#[error("no stream is left to copy")]
	NothingKept,
	/// Video only, but the input has no video.
	#[error("the input has no video stream")]
	NoVideo,
}

/// A selection resolved against an input: what the output will hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
	/// The target container.
	pub container: Container,
	/// See [`Selection::mode`].
	pub mode: Mode,
	/// One decision per stream of the input, in container order.
	pub decisions: Vec<Decision>,
	/// The kept streams the container cannot or may not hold, in stream
	/// order.
	pub issues: Vec<Issue>,
}

impl Plan {
	/// The indexes of the kept streams, in container order.
	pub fn kept(&self) -> impl Iterator<Item = u32> + '_ {
		self.decisions.iter().filter(|d| d.kept).map(|d| d.index)
	}

	/// Whether every stream of the input is kept.
	#[must_use]
	pub fn keeps_everything(&self) -> bool {
		self.decisions.iter().all(|d| d.kept)
	}

	/// Whether no blocking issue stands in the way of running the copy.
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

	/// The ffmpeg command that copies the kept streams of `input` into
	/// `output`; see [`command::copy_streams`].
	#[must_use]
	pub fn command(&self, input: &Path, output: &Path) -> FfmpegCommand {
		command::copy_streams(input, output, self)
	}
}

/// Resolves `selection` against the streams of `probe` for `container`.
///
/// # Errors
/// See [`PlanError`]: the selection names an unknown stream, keeps nothing,
/// or asks for video from a file without any. A stream the container
/// cannot hold is not an error here but an [`Issue`] on the plan.
pub fn plan(
	probe: &ProbeResult,
	selection: &Selection,
	container: Container,
) -> Result<Plan, PlanError> {
	if matches!(selection.mode, Mode::Keep | Mode::Drop)
		&& let Some(unknown) = selection
			.streams
			.iter()
			.find(|i| !probe.streams.iter().any(|s| s.index == **i))
	{
		return Err(PlanError::UnknownStream(*unknown));
	}
	let decisions: Vec<Decision> = probe
		.streams
		.iter()
		.map(|stream| decide(stream, selection))
		.collect();
	if !decisions.iter().any(|d| d.kept) {
		return Err(if selection.mode == Mode::VideoOnly {
			PlanError::NoVideo
		} else {
			PlanError::NothingKept
		});
	}
	let issues = probe
		.streams
		.iter()
		.zip(&decisions)
		.filter(|(_, decision)| decision.kept)
		.filter_map(|(stream, _)| issue(stream, container))
		.collect();
	Ok(Plan {
		container,
		mode: selection.mode,
		decisions,
		issues,
	})
}

fn decide(stream: &Stream, selection: &Selection) -> Decision {
	let listed = selection.streams.contains(&stream.index);
	let (kept, reason) = match selection.mode {
		Mode::All => (true, Reason::All),
		Mode::Keep if listed => (true, Reason::Ticked),
		Mode::Keep => (false, Reason::Unticked),
		Mode::Drop if listed => (false, Reason::Unticked),
		Mode::Drop => (true, Reason::Ticked),
		Mode::VideoOnly => match stream.codec_type {
			CodecType::Video if stream.disposition.attached_pic => (false, Reason::CoverArt),
			CodecType::Video => (true, Reason::Video),
			_ => (false, Reason::NotVideo),
		},
	};
	Decision {
		index: stream.index,
		kept,
		reason,
	}
}

/// The issue of keeping `stream` in `container`, if any.
fn issue(stream: &Stream, container: Container) -> Option<Issue> {
	let what = Container::describe(stream);
	let index = stream.index;
	match container.holds(stream) {
		Compatibility::Supported => None,
		Compatibility::Unsupported => {
			let others: Vec<&str> = CONTAINERS
				.into_iter()
				.filter(|c| *c != container && c.holds(stream) == Compatibility::Supported)
				.map(Container::name)
				.collect();
			let advice = if others.is_empty() {
				format!("drop stream {index}")
			} else {
				format!("drop stream {index} or choose {}", others.join(" or "))
			};
			Some(Issue {
				index,
				blocking: true,
				message: format!("{container} cannot hold {what} (stream {index}); {advice}"),
			})
		}
		Compatibility::Uncertain => Some(Issue {
			index,
			blocking: false,
			message: format!(
				"{container} may not hold {what} (stream {index}); ffmpeg decides when the job runs"
			),
		}),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe::parse;

	/// Video, two audio tracks, a subtitle, cover art and a font.
	fn movie() -> ProbeResult {
		parse(
			r#"{"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264"},
				{"index":1,"codec_type":"audio","codec_name":"aac","tags":{"language":"eng"}},
				{"index":2,"codec_type":"audio","codec_name":"truehd","tags":{"language":"fre"}},
				{"index":3,"codec_type":"subtitle","codec_name":"subrip"},
				{"index":4,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}},
				{"index":5,"codec_type":"attachment"}
			]}"#,
		)
		.unwrap()
	}

	fn kept(plan: &Plan) -> Vec<u32> {
		plan.kept().collect()
	}

	#[test]
	fn all_keeps_every_stream() {
		let plan = plan(&movie(), &Selection::all(), Container::Mkv).unwrap();
		assert_eq!(kept(&plan), [0, 1, 2, 3, 4, 5]);
		assert!(plan.keeps_everything());
		assert_eq!(plan.issues, Vec::<Issue>::new());
		assert!(plan.can_run());
		assert!(plan.decisions.iter().all(|d| d.reason == Reason::All));
	}

	#[test]
	fn keep_and_drop_mirror_each_other() {
		let keep = plan(&movie(), &Selection::keep([0, 1]), Container::Mkv).unwrap();
		assert_eq!(kept(&keep), [0, 1]);
		assert_eq!(keep.decisions[1].reason, Reason::Ticked);
		assert_eq!(keep.decisions[2].reason, Reason::Unticked);
		assert!(!keep.keeps_everything());

		let drop = plan(&movie(), &Selection::drop([2, 3, 4, 5]), Container::Mkv).unwrap();
		assert_eq!(kept(&drop), [0, 1]);
		assert_eq!(drop.decisions[0].reason, Reason::Ticked);
		assert_eq!(drop.decisions[3].reason, Reason::Unticked);
	}

	#[test]
	fn video_only_leaves_out_cover_art() {
		let plan = plan(&movie(), &Selection::video_only(), Container::Mp4).unwrap();
		assert_eq!(kept(&plan), [0]);
		assert_eq!(plan.decisions[0].reason, Reason::Video);
		assert_eq!(plan.decisions[1].reason, Reason::NotVideo);
		assert_eq!(plan.decisions[4].reason, Reason::CoverArt);
		assert!(plan.can_run());
	}

	#[test]
	fn impossible_selections_are_errors() {
		let bad = |s: Selection, c| plan(&movie(), &s, c).unwrap_err();
		assert_eq!(
			bad(Selection::keep([9]), Container::Mkv),
			PlanError::UnknownStream(9)
		);
		assert_eq!(
			bad(Selection::drop([0, 9]), Container::Mkv),
			PlanError::UnknownStream(9)
		);
		assert_eq!(
			bad(Selection::keep([]), Container::Mkv),
			PlanError::NothingKept
		);
		assert_eq!(
			bad(Selection::drop([0, 1, 2, 3, 4, 5]), Container::Mkv),
			PlanError::NothingKept
		);
		let audio =
			parse(r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"aac"}]}"#).unwrap();
		assert_eq!(
			plan(&audio, &Selection::video_only(), Container::Mp4).unwrap_err(),
			PlanError::NoVideo
		);
		assert_eq!(
			PlanError::NothingKept.to_string(),
			"no stream is left to copy"
		);
	}

	#[test]
	fn the_container_reports_what_it_cannot_hold() {
		let plan = plan(&movie(), &Selection::all(), Container::Mp4).unwrap();
		let indexes: Vec<u32> = plan.issues.iter().map(|i| i.index).collect();
		assert_eq!(indexes, [2, 3, 5]);
		assert!(plan.issues.iter().all(|i| i.blocking));
		assert!(!plan.can_run());
		assert_eq!(
			plan.issues[0].message,
			"MP4 cannot hold truehd audio (stream 2); drop stream 2 or choose MKV"
		);
		assert_eq!(
			plan.issues[1].message,
			"MP4 cannot hold subrip subtitles (stream 3); drop stream 3 or choose MKV"
		);
		assert_eq!(
			plan.issues[2].message,
			"MP4 cannot hold attachments (stream 5); drop stream 5 or choose MKV"
		);
		assert_eq!(plan.blocking_messages().len(), 3);
		assert_eq!(plan.issues[1].to_string(), plan.issues[1].message);

		// Dropping the offenders clears the issues.
		let fixed = super::plan(&movie(), &Selection::drop([2, 3, 5]), Container::Mp4).unwrap();
		assert_eq!(fixed.issues, Vec::<Issue>::new());
		assert!(fixed.can_run());
	}

	#[test]
	fn an_uncertain_codec_is_a_warning_not_a_refusal() {
		let odd =
			parse(r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"cfhd"}]}"#).unwrap();
		let plan = plan(&odd, &Selection::all(), Container::Mp4).unwrap();
		assert_eq!(plan.issues.len(), 1);
		assert!(!plan.issues[0].blocking);
		assert!(plan.can_run());
		assert_eq!(plan.blocking_messages(), Vec::<String>::new());
		assert_eq!(
			plan.issues[0].message,
			"MP4 may not hold cfhd video (stream 0); ffmpeg decides when the job runs"
		);
	}

	#[test]
	fn a_stream_no_container_holds_gets_no_alternative() {
		let data = parse(
			r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264"},{"index":1,"codec_type":"data"}]}"#,
		)
		.unwrap();
		let plan = plan(&data, &Selection::all(), Container::Mkv).unwrap();
		assert_eq!(
			plan.issues[0].message,
			"MKV cannot hold data (stream 1); drop stream 1"
		);
	}

	#[test]
	fn reasons_read_as_the_why_column() {
		assert_eq!(Reason::All.to_string(), "all streams");
		assert_eq!(Reason::Unticked.to_string(), "unticked");
		assert_eq!(Reason::CoverArt.to_string(), "cover art, not video");
	}
}
