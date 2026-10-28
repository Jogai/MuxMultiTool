//! Rule-based stream filters: drop French audio and subtitles, drop DTS
//! audio, keep English audio only. A [`Rules`] set resolves against any
//! file's streams into a [`Selection`], so the same rules serve a preset or
//! a batch, and the "why" of every stream names the rule that decided it.
//!
//! # Order of evaluation
//!
//! Every stream of the input is decided on its own, in these steps; the
//! first step that applies wins:
//!
//! 1. A stream that matches any [`Action::Drop`] rule is dropped. A drop
//!    rule always wins over a keep rule.
//! 2. Otherwise, when [`Action::Keep`] rules *cover* the stream (a keep
//!    rule covers the streams of its [`stream_type`](Rule::stream_type), or
//!    every stream when it has none), the stream is kept only if it
//!    matches one of them. Keep rules are keep-only: "keep English audio"
//!    drops every other audio stream and touches nothing else.
//! 3. A stream without a language tag (no tag, or `und`) never matches a
//!    [`Matcher::Language`], neither to drop nor to keep. When a keep-only
//!    language rule covers such a stream and no other keep rule matches
//!    it, [`Rules::untagged`] decides whether it stays.
//! 4. A stream no rule covers is kept.
//!
//! The rules' order in the list never changes the result: rules of the
//! same action are a set. Languages are matched case-insensitively, with
//! the bibliographic and terminologic ISO 639-2 codes (`fre`/`fra`,
//! `ger`/`deu`) treated as the same language; codecs are matched against
//! ffprobe's `codec_name` case-insensitively; stream ids are the index of
//! the stream in its file. A rule that matches nothing in a file is not an
//! error, since the same rules apply to many files.
//!
//! The resolved selection goes through [`plan`](super::plan) like a ticked
//! one, so the container checks and the command are the same. On top of
//! that a [`RulePlan`] carries a [`Warning`] when the rules drop every
//! video or every audio stream of the input, which is usually a mistake.

use std::fmt;

use super::{Container, Plan, PlanError, Selection};
use crate::ffmpeg::probe::{CodecType, ProbeResult, Stream};

/// What a rule does with the streams it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
	/// Drop every matching stream.
	Drop,
	/// Keep only the matching streams of the rule's type.
	Keep,
}

impl fmt::Display for Action {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::Drop => "drop",
			Self::Keep => "keep",
		})
	}
}

/// What a rule looks at in a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matcher {
	/// The `language` tag, e.g. `fre`. A stream without one never matches.
	Language(String),
	/// ffprobe's `codec_name`, e.g. `dts`.
	Codec(String),
	/// The stream's index in its file.
	Stream(u32),
}

impl Matcher {
	fn matches(&self, stream: &Stream) -> bool {
		match self {
			Self::Language(language) => {
				tagged_language(stream).is_some_and(|tag| same_language(tag, language))
			}
			Self::Codec(codec) => stream
				.codec_name
				.as_deref()
				.is_some_and(|name| name.eq_ignore_ascii_case(codec.trim())),
			Self::Stream(index) => stream.index == *index,
		}
	}

	fn value_is_empty(&self) -> bool {
		match self {
			Self::Language(value) | Self::Codec(value) => value.trim().is_empty(),
			Self::Stream(_) => false,
		}
	}
}

/// One filter: an action, the stream type it applies to and what it
/// matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
	/// See [`Action`].
	pub action: Action,
	/// The streams the rule looks at: those of one type, or every stream
	/// when `None`.
	pub stream_type: Option<CodecType>,
	/// See [`Matcher`].
	pub matcher: Matcher,
}

impl Rule {
	/// A rule that drops the matching streams of `stream_type`.
	#[must_use]
	pub const fn drop(stream_type: Option<CodecType>, matcher: Matcher) -> Self {
		Self {
			action: Action::Drop,
			stream_type,
			matcher,
		}
	}

	/// A rule that keeps only the matching streams of `stream_type`.
	#[must_use]
	pub const fn keep(stream_type: Option<CodecType>, matcher: Matcher) -> Self {
		Self {
			action: Action::Keep,
			stream_type,
			matcher,
		}
	}

	/// Whether the rule looks at `stream` at all.
	fn covers(&self, stream: &Stream) -> bool {
		self.stream_type.is_none_or(|t| t == stream.codec_type)
	}

	fn matches(&self, stream: &Stream) -> bool {
		self.covers(stream) && self.matcher.matches(stream)
	}
}

impl fmt::Display for Rule {
	/// The short name the "why" column uses: `drop audio fre`, `keep eng`,
	/// `drop stream 2`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.action)?;
		if let Some(stream_type) = self.stream_type {
			write!(f, " {stream_type}")?;
		}
		match &self.matcher {
			Matcher::Language(value) | Matcher::Codec(value) => write!(f, " {}", value.trim()),
			Matcher::Stream(index) => write!(f, " stream {index}"),
		}
	}
}

/// What a keep-only language rule does with a stream of its type that has
/// no language tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Untagged {
	/// Keep it: its language is unknown, so it may well be the wanted one.
	#[default]
	Keep,
	/// Drop it: only streams known to be in the wanted language stay.
	Drop,
}

/// A set of rules and how they treat untagged streams.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rules {
	/// The rules, in any order.
	pub rules: Vec<Rule>,
	/// See [`Untagged`].
	pub untagged: Untagged,
}

/// Why rules cannot be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
	/// A language or codec rule has nothing to match.
	#[error("rule {0} has an empty value")]
	EmptyValue(usize),
	/// The resolved selection cannot be planned: nothing is left.
	#[error(transparent)]
	Plan(#[from] PlanError),
}

/// What the rules do with one stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
	/// The stream's index.
	pub index: u32,
	/// Whether it is copied to the output.
	pub kept: bool,
	/// Which rule decided it, for the "why" column, e.g. `rule: drop audio
	/// fre, drop audio dts`.
	pub why: String,
}

/// A result that is usually a mistake, but not an error: the user may
/// mean it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
	/// The input has video and the rules drop all of it.
	NoVideo,
	/// The input has audio and the rules drop all of it.
	NoAudio,
}

impl fmt::Display for Warning {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::NoVideo => "the rules drop every video stream of the input",
			Self::NoAudio => "the rules drop every audio stream of the input",
		})
	}
}

/// The rules resolved against one file's streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
	/// One decision per stream of the input, in container order.
	pub decisions: Vec<Decision>,
	/// See [`Warning`].
	pub warnings: Vec<Warning>,
}

impl Resolution {
	/// The selection the rules resolved to: the kept streams, ticked.
	#[must_use]
	pub fn selection(&self) -> Selection {
		Selection::keep(self.decisions.iter().filter(|d| d.kept).map(|d| d.index))
	}
}

impl Rules {
	/// Checks that every rule has something to match.
	///
	/// # Errors
	/// [`Error::EmptyValue`] names the first rule (0-based) with an empty
	/// language or codec.
	pub fn validate(&self) -> Result<(), Error> {
		match self.rules.iter().position(|r| r.matcher.value_is_empty()) {
			Some(rule) => Err(Error::EmptyValue(rule)),
			None => Ok(()),
		}
	}

	/// Decides every stream of `probe`; see the module docs for the order
	/// of evaluation.
	///
	/// # Errors
	/// See [`validate`](Self::validate).
	pub fn resolve(&self, probe: &ProbeResult) -> Result<Resolution, Error> {
		self.validate()?;
		let decisions: Vec<Decision> = probe.streams.iter().map(|s| self.decide(s)).collect();
		let mut warnings = Vec::new();
		let drops_every = |codec_type: CodecType| {
			let mut of_type = probe
				.streams
				.iter()
				.zip(&decisions)
				.filter(|(s, _)| s.codec_type == codec_type && !s.disposition.attached_pic)
				.peekable();
			of_type.peek().is_some() && of_type.all(|(_, d)| !d.kept)
		};
		if drops_every(CodecType::Video) {
			warnings.push(Warning::NoVideo);
		}
		if drops_every(CodecType::Audio) {
			warnings.push(Warning::NoAudio);
		}
		Ok(Resolution {
			decisions,
			warnings,
		})
	}

	fn decide(&self, stream: &Stream) -> Decision {
		let index = stream.index;
		let names = |rules: &[&Rule]| {
			rules
				.iter()
				.map(ToString::to_string)
				.collect::<Vec<_>>()
				.join(", ")
		};
		let dropped_by: Vec<&Rule> = self
			.rules
			.iter()
			.filter(|r| r.action == Action::Drop && r.matches(stream))
			.collect();
		if !dropped_by.is_empty() {
			return Decision {
				index,
				kept: false,
				why: format!("rule: {}", names(&dropped_by)),
			};
		}
		let covering: Vec<&Rule> = self
			.rules
			.iter()
			.filter(|r| r.action == Action::Keep && r.covers(stream))
			.collect();
		if covering.is_empty() {
			return Decision {
				index,
				kept: true,
				why: "no rule applies".into(),
			};
		}
		let kept_by: Vec<&Rule> = covering
			.iter()
			.copied()
			.filter(|r| r.matcher.matches(stream))
			.collect();
		if !kept_by.is_empty() {
			return Decision {
				index,
				kept: true,
				why: format!("rule: {}", names(&kept_by)),
			};
		}
		let by_language = covering
			.iter()
			.any(|r| matches!(r.matcher, Matcher::Language(_)));
		if by_language && tagged_language(stream).is_none() {
			return match self.untagged {
				Untagged::Keep => Decision {
					index,
					kept: true,
					why: "no language tag, kept".into(),
				},
				Untagged::Drop => Decision {
					index,
					kept: false,
					why: "no language tag, dropped".into(),
				},
			};
		}
		Decision {
			index,
			kept: false,
			why: format!("not kept by rule: {}", names(&covering)),
		}
	}
}

/// Rules resolved and planned: the plan of the selection they resolved
/// to, with the rules' own "why" per stream and warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulePlan {
	/// The plan of the resolved selection, with the container's issues and
	/// the command. Its decisions' reasons say "ticked" and "unticked";
	/// [`decisions`](Self::decisions) say which rule did it.
	pub plan: Plan,
	/// One decision per stream of the input, in container order.
	pub decisions: Vec<Decision>,
	/// See [`Warning`].
	pub warnings: Vec<Warning>,
}

/// Resolves `rules` against the streams of `probe` and plans the result
/// for `container`; see [`plan`](super::plan).
///
/// # Errors
/// A rule with an empty value, or a resolution that keeps nothing.
pub fn plan(probe: &ProbeResult, rules: &Rules, container: Container) -> Result<RulePlan, Error> {
	let resolution = rules.resolve(probe)?;
	let plan = super::plan(probe, &resolution.selection(), container)?;
	Ok(RulePlan {
		plan,
		decisions: resolution.decisions,
		warnings: resolution.warnings,
	})
}

/// The stream's language tag, or `None` when it has none or it is `und`.
fn tagged_language(stream: &Stream) -> Option<&str> {
	stream
		.language()
		.map(str::trim)
		.filter(|tag| !tag.is_empty() && !tag.eq_ignore_ascii_case("und"))
}

/// Whether two ISO 639 codes name the same language, taking the
/// bibliographic/terminologic pairs of ISO 639-2 as equal.
fn same_language(a: &str, b: &str) -> bool {
	let a = a.trim().to_ascii_lowercase();
	let b = b.trim().to_ascii_lowercase();
	a == b
		|| ISO_639_2_B_T
			.iter()
			.any(|(x, y)| (a == *x && b == *y) || (a == *y && b == *x))
}

/// The ISO 639-2 languages with a bibliographic and a terminologic code.
const ISO_639_2_B_T: [(&str, &str); 20] = [
	("alb", "sqi"),
	("arm", "hye"),
	("baq", "eus"),
	("bur", "mya"),
	("chi", "zho"),
	("cze", "ces"),
	("dut", "nld"),
	("fre", "fra"),
	("geo", "kat"),
	("ger", "deu"),
	("gre", "ell"),
	("ice", "isl"),
	("mac", "mkd"),
	("mao", "mri"),
	("may", "msa"),
	("per", "fas"),
	("rum", "ron"),
	("slo", "slk"),
	("tib", "bod"),
	("wel", "cym"),
];

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe::parse;
	use crate::streams::Reason;

	/// The sketch's Movie A: video, three audio tracks (`TrueHD` eng, DTS
	/// fre, AC-3 eng commentary), an untagged AAC track, three subtitles
	/// (PGS eng, PGS fre, SubRip untagged) and cover art.
	fn movie() -> ProbeResult {
		parse(
			r#"{"streams":[
				{"index":0,"codec_type":"video","codec_name":"hevc","tags":{"language":"und"}},
				{"index":1,"codec_type":"audio","codec_name":"truehd","tags":{"language":"eng"}},
				{"index":2,"codec_type":"audio","codec_name":"dts","tags":{"language":"fre"}},
				{"index":3,"codec_type":"audio","codec_name":"ac3","tags":{"language":"eng"}},
				{"index":4,"codec_type":"audio","codec_name":"aac"},
				{"index":5,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle","tags":{"language":"eng"}},
				{"index":6,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle","tags":{"language":"fre"}},
				{"index":7,"codec_type":"subtitle","codec_name":"subrip"},
				{"index":8,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}
			]}"#,
		)
		.unwrap()
	}

	fn language(value: &str) -> Matcher {
		Matcher::Language(value.into())
	}

	fn codec(value: &str) -> Matcher {
		Matcher::Codec(value.into())
	}

	fn rules(rules: impl IntoIterator<Item = Rule>) -> Rules {
		Rules {
			rules: rules.into_iter().collect(),
			untagged: Untagged::Keep,
		}
	}

	fn resolve(rules: &Rules) -> Resolution {
		rules.resolve(&movie()).unwrap()
	}

	fn kept(resolution: &Resolution) -> Vec<u32> {
		resolution
			.decisions
			.iter()
			.filter(|d| d.kept)
			.map(|d| d.index)
			.collect()
	}

	fn why(resolution: &Resolution, index: usize) -> &str {
		&resolution.decisions[index].why
	}

	#[test]
	fn no_rules_keep_everything() {
		let resolution = resolve(&Rules::default());
		assert_eq!(kept(&resolution), [0, 1, 2, 3, 4, 5, 6, 7, 8]);
		assert!(
			resolution
				.decisions
				.iter()
				.all(|d| d.why == "no rule applies")
		);
		assert_eq!(resolution.warnings, Vec::<Warning>::new());
		assert_eq!(resolution.selection(), Selection::keep(0..=8));
	}

	#[test]
	fn drop_by_language_drops_french_audio_and_subtitles() {
		let resolution = resolve(&rules([
			Rule::drop(Some(CodecType::Audio), language("fre")),
			Rule::drop(Some(CodecType::Subtitle), language("fre")),
		]));
		assert_eq!(kept(&resolution), [0, 1, 3, 4, 5, 7, 8]);
		assert_eq!(why(&resolution, 2), "rule: drop audio fre");
		assert_eq!(why(&resolution, 6), "rule: drop subtitle fre");
		assert_eq!(why(&resolution, 1), "no rule applies");
		assert_eq!(resolution.warnings, Vec::<Warning>::new());

		// Without a type the rule drops French anything.
		let resolution = resolve(&rules([Rule::drop(None, language("fre"))]));
		assert_eq!(kept(&resolution), [0, 1, 3, 4, 5, 7, 8]);
		assert_eq!(why(&resolution, 2), "rule: drop fre");
	}

	#[test]
	fn languages_match_loosely() {
		// Case, surrounding spaces, and the ISO 639-2 B/T pairs.
		for value in ["FRE", " fre ", "fra", "Fra"] {
			let resolution = resolve(&rules([Rule::drop(None, language(value))]));
			assert_eq!(kept(&resolution), [0, 1, 3, 4, 5, 7, 8], "{value:?}");
		}
		assert!(same_language("deu", "ger"));
		assert!(same_language("ger", "deu"));
		assert!(!same_language("eng", "fre"));
		assert!(!same_language("fre", "ger"));
	}

	#[test]
	fn drop_by_codec_or_stream_id() {
		// Matched case-insensitively, named as the user wrote it.
		let resolution = resolve(&rules([Rule::drop(Some(CodecType::Audio), codec("DTS"))]));
		assert_eq!(kept(&resolution), [0, 1, 3, 4, 5, 6, 7, 8]);
		assert_eq!(why(&resolution, 2), "rule: drop audio DTS");

		// A codec rule of a type that has no such codec drops nothing.
		let resolution = resolve(&rules([Rule::drop(Some(CodecType::Video), codec("dts"))]));
		assert_eq!(kept(&resolution).len(), 9);

		let resolution = resolve(&rules([
			Rule::drop(None, Matcher::Stream(3)),
			Rule::drop(None, Matcher::Stream(42)),
		]));
		assert_eq!(kept(&resolution), [0, 1, 2, 4, 5, 6, 7, 8]);
		assert_eq!(why(&resolution, 3), "rule: drop stream 3");

		// A stream id rule respects the type too.
		let resolution = resolve(&rules([Rule::drop(
			Some(CodecType::Subtitle),
			Matcher::Stream(3),
		)]));
		assert_eq!(kept(&resolution).len(), 9);
	}

	#[test]
	fn keep_only_discards_the_other_languages_of_its_type() {
		let resolution = resolve(&rules([Rule::keep(
			Some(CodecType::Audio),
			language("eng"),
		)]));
		// Video, subtitles and cover art are untouched; the untagged audio
		// stays by default.
		assert_eq!(kept(&resolution), [0, 1, 3, 4, 5, 6, 7, 8]);
		assert_eq!(why(&resolution, 1), "rule: keep audio eng");
		assert_eq!(why(&resolution, 2), "not kept by rule: keep audio eng");
		assert_eq!(why(&resolution, 4), "no language tag, kept");
		assert_eq!(why(&resolution, 5), "no rule applies");

		// Two keep rules of a type are alternatives.
		let resolution = resolve(&rules([
			Rule::keep(Some(CodecType::Audio), language("eng")),
			Rule::keep(Some(CodecType::Audio), codec("dts")),
		]));
		assert_eq!(kept(&resolution), [0, 1, 2, 3, 4, 5, 6, 7, 8]);
		assert_eq!(why(&resolution, 2), "rule: keep audio dts");

		// A keep rule without a type covers every stream, video included:
		// the warning says so.
		let resolution = resolve(&rules([Rule::keep(None, language("eng"))]));
		assert_eq!(kept(&resolution), [0, 1, 3, 4, 5, 7, 8]);
		assert_eq!(why(&resolution, 0), "no language tag, kept");
		assert_eq!(why(&resolution, 6), "not kept by rule: keep eng");
	}

	#[test]
	fn untagged_streams_follow_the_setting() {
		let mut english = rules([
			Rule::keep(Some(CodecType::Audio), language("eng")),
			Rule::keep(Some(CodecType::Subtitle), language("eng")),
		]);
		let resolution = english.resolve(&movie()).unwrap();
		assert_eq!(kept(&resolution), [0, 1, 3, 4, 5, 7, 8]);
		assert_eq!(why(&resolution, 4), "no language tag, kept");
		assert_eq!(why(&resolution, 7), "no language tag, kept");

		english.untagged = Untagged::Drop;
		let resolution = english.resolve(&movie()).unwrap();
		assert_eq!(kept(&resolution), [0, 1, 3, 5, 8]);
		assert_eq!(why(&resolution, 4), "no language tag, dropped");
		assert_eq!(why(&resolution, 7), "no language tag, dropped");

		// The setting only matters for keep-only language rules: a drop by
		// language never matches an untagged stream, and a keep by codec
		// decides untagged streams like any other.
		let resolution = Rules {
			rules: vec![Rule::drop(Some(CodecType::Audio), language("eng"))],
			untagged: Untagged::Drop,
		}
		.resolve(&movie())
		.unwrap();
		assert_eq!(kept(&resolution), [0, 2, 4, 5, 6, 7, 8]);
		let resolution = Rules {
			rules: vec![Rule::keep(Some(CodecType::Audio), codec("aac"))],
			untagged: Untagged::Drop,
		}
		.resolve(&movie())
		.unwrap();
		assert_eq!(kept(&resolution), [0, 4, 5, 6, 7, 8]);
		assert_eq!(why(&resolution, 1), "not kept by rule: keep audio aac");

		// `und` and a missing tag are both untagged; a drop of `und` matches
		// nothing.
		let resolution = resolve(&rules([Rule::drop(None, language("und"))]));
		assert_eq!(kept(&resolution).len(), 9);
	}

	#[test]
	fn rules_combine_with_drop_before_keep() {
		// The sketch's four rules, in an order that would differ if it
		// mattered.
		let combined = [
			Rule::keep(Some(CodecType::Audio), language("eng")),
			Rule::drop(Some(CodecType::Audio), codec("dts")),
			Rule::drop(Some(CodecType::Subtitle), language("fre")),
			Rule::drop(Some(CodecType::Audio), language("fre")),
		];
		let resolution = resolve(&rules(combined.clone()));
		assert_eq!(kept(&resolution), [0, 1, 3, 4, 5, 7, 8]);
		// Every drop rule that matched is named, in rule order.
		assert_eq!(why(&resolution, 2), "rule: drop audio dts, drop audio fre");
		assert_eq!(why(&resolution, 6), "rule: drop subtitle fre");
		assert_eq!(why(&resolution, 1), "rule: keep audio eng");
		assert_eq!(why(&resolution, 3), "rule: keep audio eng");

		let mut reversed = combined;
		reversed.reverse();
		let again = resolve(&rules(reversed));
		assert_eq!(kept(&again), kept(&resolution));
		assert_eq!(why(&again, 2), "rule: drop audio fre, drop audio dts");

		// A drop wins over a keep that matches the same stream.
		let resolution = resolve(&rules([
			Rule::keep(Some(CodecType::Audio), language("eng")),
			Rule::drop(Some(CodecType::Audio), codec("truehd")),
		]));
		assert_eq!(kept(&resolution), [0, 3, 4, 5, 6, 7, 8]);
		assert_eq!(why(&resolution, 1), "rule: drop audio truehd");
	}

	#[test]
	fn dropping_all_video_or_audio_warns() {
		let resolution = resolve(&rules([Rule::drop(
			Some(CodecType::Audio),
			codec("truehd"),
		)]));
		assert_eq!(resolution.warnings, Vec::<Warning>::new());

		// Video only: the warning is about the audio, and cover art does
		// not count as video.
		let resolution = resolve(&rules([
			Rule::drop(Some(CodecType::Audio), language("eng")),
			Rule::drop(Some(CodecType::Audio), language("fre")),
			Rule::drop(Some(CodecType::Audio), codec("aac")),
		]));
		assert_eq!(kept(&resolution), [0, 5, 6, 7, 8]);
		assert_eq!(resolution.warnings, [Warning::NoAudio]);

		let resolution = resolve(&rules([Rule::drop(Some(CodecType::Video), codec("hevc"))]));
		assert_eq!(resolution.warnings, [Warning::NoVideo]);
		assert_eq!(
			resolution.warnings[0].to_string(),
			"the rules drop every video stream of the input"
		);

		// Keep-only with a language no stream has drops both.
		let resolution = resolve(&Rules {
			rules: vec![Rule::keep(None, language("jpn"))],
			untagged: Untagged::Drop,
		});
		assert_eq!(kept(&resolution), Vec::<u32>::new());
		assert_eq!(resolution.warnings, [Warning::NoVideo, Warning::NoAudio]);

		// No warning about a type the input does not have.
		let audio =
			parse(r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"aac"}]}"#).unwrap();
		let resolution = rules([Rule::drop(None, codec("h264"))])
			.resolve(&audio)
			.unwrap();
		assert_eq!(resolution.warnings, Vec::<Warning>::new());
	}

	#[test]
	fn empty_values_are_refused() {
		let bad = rules([
			Rule::drop(None, codec("dts")),
			Rule::keep(Some(CodecType::Audio), language("  ")),
		]);
		assert_eq!(bad.validate(), Err(Error::EmptyValue(1)));
		assert_eq!(bad.resolve(&movie()).unwrap_err(), Error::EmptyValue(1));
		assert_eq!(
			Error::EmptyValue(1).to_string(),
			"rule 1 has an empty value"
		);
		assert_eq!(
			rules([Rule::drop(None, Matcher::Stream(0))]).validate(),
			Ok(())
		);
	}

	#[test]
	fn rules_read_as_the_why_column() {
		assert_eq!(
			Rule::drop(Some(CodecType::Audio), language(" fre ")).to_string(),
			"drop audio fre"
		);
		assert_eq!(Rule::keep(None, language("eng")).to_string(), "keep eng");
		assert_eq!(
			Rule::drop(Some(CodecType::Subtitle), Matcher::Stream(5)).to_string(),
			"drop subtitle stream 5"
		);
		assert_eq!(Action::Keep.to_string(), "keep");
		assert_eq!(Untagged::default(), Untagged::Keep);
	}

	#[test]
	fn the_plan_follows_the_resolved_selection() {
		let french = rules([
			Rule::drop(None, language("fre")),
			Rule::drop(Some(CodecType::Audio), codec("dts")),
		]);
		let planned = plan(&movie(), &french, Container::Mkv).unwrap();
		assert_eq!(
			planned.plan.kept().collect::<Vec<_>>(),
			[0, 1, 3, 4, 5, 7, 8]
		);
		assert_eq!(planned.plan.decisions[2].reason, Reason::Unticked);
		assert_eq!(planned.decisions[2].why, "rule: drop fre, drop audio dts");
		assert!(planned.plan.can_run());
		assert_eq!(planned.warnings, Vec::<Warning>::new());

		// The container's checks still apply to what the rules keep.
		let planned = plan(&movie(), &french, Container::Mp4).unwrap();
		assert!(!planned.plan.can_run());
		assert!(planned.plan.issues.iter().any(|i| i.index == 1));

		// Keeping nothing is the plan's error.
		let nothing = Rules {
			rules: vec![Rule::keep(None, language("jpn"))],
			untagged: Untagged::Drop,
		};
		assert_eq!(
			plan(&movie(), &nothing, Container::Mkv).unwrap_err(),
			Error::Plan(PlanError::NothingKept)
		);
		assert_eq!(
			Error::Plan(PlanError::NothingKept).to_string(),
			"no stream is left to copy"
		);
	}
}
