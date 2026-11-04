//! Batch operations: one stream operation applied to every matching file
//! of a folder, each file as its own job.
//!
//! The motivating case is "every MP4 in this folder to MKV in that one".
//! More generally a [`Batch`] names the stream operation (a remux, ticked
//! streams or [`Rules`], which resolve per file), where the outputs go
//! ([`Destination`]), how each output is named (a [`Template`]) and what to
//! do with an output that already exists ([`Existing`]). The files come
//! from [`MediaRoots::find`](crate::files::MediaRoots::find) with a
//! wildcard; the caller probes them and hands [`plan`] one [`Input`] per
//! file.
//!
//! [`plan`] decides every file on its own and never stops at one: it
//! returns an [`Item`] per input, either [`Ready`] with the stream plan and
//! the command, or [`Skipped`](Outcome::Skipped) with the [`Skip`] reason.
//! That is the dry run; a submit queues one job per ready item, so a file
//! that fails does not stop the others. Nothing is ever overwritten unless
//! [`Existing::Overwrite`] was chosen, and even then an output that is an
//! input of the batch, or the file's own input, is refused. Two files that
//! render to the same name are a collision: the first keeps it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ffmpeg::FfmpegCommand;
use crate::ffmpeg::probe::{CodecType, ProbeResult};
use crate::streams::rules::Rules;
use crate::streams::{self, Container, Plan, Selection};

pub mod template;

pub use template::{Field, Fields, PLACEHOLDERS, Template, TemplateError, utc_date};

/// What to do when an output file already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Existing {
	/// Leave the file alone and skip the input: the default.
	#[default]
	Skip,
	/// Replace it: the job runs ffmpeg with `-y`.
	Overwrite,
}

/// Where the outputs go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
	/// One folder for every output.
	Folder(PathBuf),
	/// Each output next to its input, so a recursive batch keeps the tree.
	NextToInput,
}

/// Which streams of every file are copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Streams {
	/// The same selection for every file: all streams (a remux), the
	/// video only, or stream indexes to keep or drop. An index a file does
	/// not have skips that file.
	Selection(Selection),
	/// Rules resolved against each file's own streams.
	Rules(Rules),
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
	/// See [`Destination`].
	pub destination: Destination,
	/// The output name template; its extension picks the container.
	pub template: Template,
	/// See [`Existing`].
	pub existing: Existing,
	/// See [`Streams`].
	pub streams: Streams,
}

/// One file found for the batch, with what ffprobe said about it, or why
/// it could not be probed.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
	/// The file's real path, canonical.
	pub path: PathBuf,
	/// The probe, or the error as text.
	pub probe: Result<ProbeResult, String>,
}

/// Why a file is left out of the batch.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Skip {
	/// ffprobe failed on the input.
	#[error("the file could not be probed: {0}")]
	Probe(String),
	/// The rendered name is empty or not a file name.
	#[error("the output name {0:?} is not a file name")]
	BadName(String),
	/// The rendered name's extension picks no container.
	#[error("the extension of {name} picks no container; use one of {extensions}")]
	NoContainer {
		/// The rendered name.
		name: String,
		/// The extensions the containers claim.
		extensions: String,
	},
	/// The output is the input itself.
	#[error("the output would be the input itself")]
	SameAsInput,
	/// The output is another input of the batch.
	#[error("the output is an input of this batch: {}", .0.display())]
	IsInput(PathBuf),
	/// The output exists and overwriting is not allowed.
	#[error("the output exists; allow overwriting to replace it")]
	Exists,
	/// Another file of the batch, planned earlier, writes the same output.
	#[error("another file of the batch has the same output name: {}", .0.display())]
	Collision(PathBuf),
	/// The selection cannot be resolved for this file: an unknown stream
	/// index, nothing kept, or a rule with an empty value.
	#[error("{0}")]
	Streams(String),
	/// The container cannot hold a kept stream.
	#[error("the {container} output cannot hold every selected stream: {problems}")]
	CannotHold {
		/// The container's name.
		container: String,
		/// The blocking issues' messages, joined with `; `.
		problems: String,
	},
}

/// A file the batch would process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
	/// The stream plan: kept and dropped streams, container issues.
	pub plan: Plan,
	/// The "why" of every stream, in the plan's order: the plan's reason,
	/// or the rule that decided it.
	pub whys: Vec<String>,
	/// The rules' warnings; empty for a ticked selection.
	pub warnings: Vec<String>,
	/// Whether an existing output is replaced.
	pub overwrites: bool,
	/// The input's duration, for the job's percentage and ETA.
	pub expected_duration: Option<Duration>,
}

/// What the batch does with one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
	/// The file runs as a job.
	Ready(Ready),
	/// The file is left out, for this reason.
	Skipped(Skip),
}

/// One input of the batch and what happens to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
	/// The input's path.
	pub input: PathBuf,
	/// The output's path, once a name could be rendered.
	pub output: Option<PathBuf>,
	/// See [`Outcome`].
	pub outcome: Outcome,
}

impl Item {
	fn skipped(input: &Path, output: Option<PathBuf>, skip: Skip) -> Self {
		Self {
			input: input.to_path_buf(),
			output,
			outcome: Outcome::Skipped(skip),
		}
	}

	/// The plan when the file runs.
	#[must_use]
	pub fn ready(&self) -> Option<&Ready> {
		match &self.outcome {
			Outcome::Ready(ready) => Some(ready),
			Outcome::Skipped(_) => None,
		}
	}

	/// Why the file is skipped.
	#[must_use]
	pub fn skip(&self) -> Option<&Skip> {
		match &self.outcome {
			Outcome::Ready(_) => None,
			Outcome::Skipped(skip) => Some(skip),
		}
	}

	/// The ffmpeg command of a ready item: the stream copy, with `-y` in
	/// front when it replaces an existing output.
	#[must_use]
	pub fn command(&self) -> Option<FfmpegCommand> {
		let ready = self.ready()?;
		let output = self.output.as_deref()?;
		let copy = ready.plan.command(&self.input, output);
		Some(if ready.overwrites {
			FfmpegCommand::new().flag("-y").args(copy.as_args())
		} else {
			copy
		})
	}
}

/// Plans `batch` over `inputs`: one [`Item`] per input, in the same order.
/// `date` fills `{date}` (see [`utc_date`]).
///
/// Every file is decided on its own; see the module docs for the checks
/// and their order.
#[must_use]
pub fn plan(batch: &Batch, inputs: &[Input], date: &str) -> Vec<Item> {
	let width = inputs.len().to_string().len();
	let input_paths: HashSet<&Path> = inputs.iter().map(|i| i.path.as_path()).collect();
	// Output path to the input that got it first.
	let mut taken: HashMap<PathBuf, PathBuf> = HashMap::new();
	inputs
		.iter()
		.enumerate()
		.map(|(i, input)| {
			let item = decide(batch, input, &input_paths, &taken, i + 1, width, date);
			if let (Some(output), Outcome::Ready(_)) = (&item.output, &item.outcome) {
				taken.insert(output.clone(), input.path.clone());
			}
			item
		})
		.collect()
}

fn decide(
	batch: &Batch,
	input: &Input,
	input_paths: &HashSet<&Path>,
	taken: &HashMap<PathBuf, PathBuf>,
	number: usize,
	width: usize,
	date: &str,
) -> Item {
	let path = input.path.as_path();
	let probe = match &input.probe {
		Ok(probe) => probe,
		Err(error) => return Item::skipped(path, None, Skip::Probe(error.clone())),
	};
	let (selection, rule_whys, warnings) = match &batch.streams {
		Streams::Selection(selection) => (selection.clone(), None, Vec::new()),
		Streams::Rules(rules) => match rules.resolve(probe) {
			Ok(resolution) => {
				let warnings = resolution
					.warnings
					.iter()
					.map(ToString::to_string)
					.collect();
				let selection = resolution.selection();
				let whys = resolution.decisions.into_iter().map(|d| d.why).collect();
				(selection, Some(whys), warnings)
			}
			Err(error) => return Item::skipped(path, None, Skip::Streams(error.to_string())),
		},
	};
	// The kept streams do not depend on the container, but `{lang}` needs
	// them before the name, and so the container, is known.
	let kept: Vec<u32> = match streams::plan(probe, &selection, Container::Mkv) {
		Ok(plan) => plan.kept().collect(),
		Err(error) => return Item::skipped(path, None, Skip::Streams(error.to_string())),
	};
	let fields = Fields {
		name: component(path.file_stem()),
		ext: component(path.extension()),
		lang: languages(probe, &kept),
		date: date.to_owned(),
		number: format!("{number:0width$}"),
		parent: component(path.parent().and_then(Path::file_name)),
	};
	let name = batch.template.render(&fields);
	if name.is_empty() || name == "." || name == ".." {
		return Item::skipped(path, None, Skip::BadName(name));
	}
	let folder = match &batch.destination {
		Destination::Folder(folder) => folder.clone(),
		Destination::NextToInput => path
			.parent()
			.map_or_else(|| PathBuf::from("/"), Path::to_path_buf),
	};
	let output = folder.join(&name);
	let Some(container) = Container::from_extension(&output) else {
		return Item::skipped(
			path,
			Some(output),
			Skip::NoContainer {
				name,
				extensions: extensions(),
			},
		);
	};
	if output == path {
		return Item::skipped(path, Some(output), Skip::SameAsInput);
	}
	if input_paths.contains(output.as_path()) {
		return Item::skipped(path, Some(output.clone()), Skip::IsInput(output));
	}
	let overwrites = output.exists();
	if overwrites && batch.existing == Existing::Skip {
		return Item::skipped(path, Some(output), Skip::Exists);
	}
	let plan = match streams::plan(probe, &selection, container) {
		Ok(plan) => plan,
		Err(error) => return Item::skipped(path, Some(output), Skip::Streams(error.to_string())),
	};
	if !plan.can_run() {
		return Item::skipped(
			path,
			Some(output),
			Skip::CannotHold {
				container: container.name().to_owned(),
				problems: plan.blocking_messages().join("; "),
			},
		);
	}
	if let Some(first) = taken.get(&output) {
		return Item::skipped(path, Some(output), Skip::Collision(first.clone()));
	}
	let whys = rule_whys.unwrap_or_else(|| {
		plan.decisions
			.iter()
			.map(|d| d.reason.to_string())
			.collect()
	});
	Item {
		input: path.to_path_buf(),
		output: Some(output),
		outcome: Outcome::Ready(Ready {
			expected_duration: probe
				.duration_secs()
				.and_then(|secs| Duration::try_from_secs_f64(secs).ok()),
			plan,
			whys,
			warnings,
			overwrites,
		}),
	}
}

/// A path component as text, empty when there is none.
fn component(part: Option<&std::ffi::OsStr>) -> String {
	part.map(|p| p.to_string_lossy().into_owned())
		.unwrap_or_default()
}

/// The distinct languages of the kept audio streams, in stream order,
/// joined with `-`; `und` when none is tagged.
fn languages(probe: &ProbeResult, kept: &[u32]) -> String {
	let mut seen = Vec::new();
	for stream in probe
		.streams
		.iter()
		.filter(|s| s.codec_type == CodecType::Audio && kept.contains(&s.index))
	{
		if let Some(language) = stream
			.language()
			.map(str::trim)
			.filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("und"))
			&& !seen.contains(&language)
		{
			seen.push(language);
		}
	}
	if seen.is_empty() {
		"und".to_owned()
	} else {
		seen.join("-")
	}
}

/// The output extensions the containers claim, for a message.
fn extensions() -> String {
	streams::CONTAINERS
		.iter()
		.flat_map(|c| c.extensions().iter().map(|e| format!(".{e}")))
		.collect::<Vec<_>>()
		.join(", ")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ffmpeg::probe::parse;
	use crate::streams::rules::{Matcher, Rule, Untagged};
	use std::fs;

	/// Video, English and French audio and a SubRip subtitle.
	fn movie() -> ProbeResult {
		parse(
			r#"{"format":{"duration":"120.5"},"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264"},
				{"index":1,"codec_type":"audio","codec_name":"aac","tags":{"language":"eng"}},
				{"index":2,"codec_type":"audio","codec_name":"dts","tags":{"language":"fre"}},
				{"index":3,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"}}
			]}"#,
		)
		.unwrap()
	}

	/// Video and untagged audio.
	fn clip() -> ProbeResult {
		parse(
			r#"{"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264"},
				{"index":1,"codec_type":"audio","codec_name":"aac"}
			]}"#,
		)
		.unwrap()
	}

	fn input(path: &str, probe: ProbeResult) -> Input {
		Input {
			path: PathBuf::from(path),
			probe: Ok(probe),
		}
	}

	fn batch(template: &str, destination: Destination) -> Batch {
		Batch {
			destination,
			template: Template::parse(template).unwrap(),
			existing: Existing::Skip,
			streams: Streams::Selection(Selection::all()),
		}
	}

	fn into(folder: &str) -> Destination {
		Destination::Folder(PathBuf::from(folder))
	}

	fn outputs(items: &[Item]) -> Vec<String> {
		items
			.iter()
			.map(|i| {
				i.output
					.as_ref()
					.map(|p| p.display().to_string())
					.unwrap_or_default()
			})
			.collect()
	}

	#[test]
	fn every_mp4_to_mkv_in_another_folder() {
		let inputs = [
			input("/media/in/a.mp4", movie()),
			input("/media/in/b.MP4", clip()),
		];
		let items = plan(
			&batch("{name}.mkv", into("/media/out")),
			&inputs,
			"2026-11-04",
		);
		assert_eq!(outputs(&items), ["/media/out/a.mkv", "/media/out/b.mkv"]);
		let a = items[0].ready().unwrap();
		assert_eq!(a.plan.container, Container::Mkv);
		assert_eq!(a.plan.kept().collect::<Vec<_>>(), [0, 1, 2, 3]);
		assert_eq!(a.whys, ["all streams"; 4]);
		assert_eq!(a.warnings, Vec::<String>::new());
		assert!(!a.overwrites);
		assert_eq!(a.expected_duration, Some(Duration::from_millis(120_500)));
		assert_eq!(items[1].ready().unwrap().expected_duration, None);
		assert_eq!(
			items[0].command().unwrap().to_string(),
			"ffmpeg -i /media/in/a.mp4 -map 0 -c copy /media/out/a.mkv"
		);
		assert_eq!(items[0].skip(), None);
	}

	#[test]
	fn the_template_fields_come_from_each_file() {
		let inputs: Vec<Input> = (1..=12)
			.map(|i| input(&format!("/media/rips/s01/e{i:02}.mkv"), movie()))
			.collect();
		let items = plan(
			&batch("{n} {parent} {name}.{lang}.{date}.{ext}", into("/out")),
			&inputs,
			"2026-11-04",
		);
		assert_eq!(outputs(&items)[0], "/out/01 s01 e01.eng-fre.2026-11-04.mkv");
		assert_eq!(
			outputs(&items)[11],
			"/out/12 s01 e12.eng-fre.2026-11-04.mkv"
		);
		assert!(items.iter().all(|i| i.ready().is_some()));

		// `{lang}` follows the kept audio: keep English only, and an
		// untagged file says `und`.
		let keep_english = Batch {
			streams: Streams::Rules(Rules {
				rules: vec![Rule::keep(
					Some(CodecType::Audio),
					Matcher::Language("eng".into()),
				)],
				untagged: Untagged::Keep,
			}),
			..batch("{name}.{lang}.mkv", into("/out"))
		};
		let items = plan(
			&keep_english,
			&[input("/in/a.mp4", movie()), input("/in/b.mp4", clip())],
			"",
		);
		assert_eq!(outputs(&items), ["/out/a.eng.mkv", "/out/b.und.mkv"]);
	}

	#[test]
	fn rules_resolve_per_file_and_a_file_that_keeps_nothing_is_skipped() {
		// Drop French audio, keep English subtitles only.
		let rules = Rules {
			rules: vec![
				Rule::drop(Some(CodecType::Audio), Matcher::Language("fre".into())),
				Rule::keep(Some(CodecType::Subtitle), Matcher::Language("eng".into())),
			],
			untagged: Untagged::Drop,
		};
		let drop_all = Rules {
			rules: vec![Rule::drop(None, Matcher::Codec("h264".into()))],
			untagged: Untagged::Keep,
		};
		let b = Batch {
			streams: Streams::Rules(rules),
			..batch("{name}.mkv", into("/out"))
		};
		let inputs = [
			input("/in/movie.mp4", movie()),
			input("/in/clip.mp4", clip()),
		];
		let items = plan(&b, &inputs, "");
		let first = items[0].ready().unwrap();
		assert_eq!(first.plan.kept().collect::<Vec<_>>(), [0, 1, 3]);
		assert_eq!(
			first.whys,
			[
				"no rule applies",
				"no rule applies",
				"rule: drop audio fre",
				"rule: keep subtitle eng"
			]
		);
		let clip = items[1].ready().unwrap();
		assert_eq!(clip.plan.kept().collect::<Vec<_>>(), [0, 1]);
		assert_eq!(clip.whys, ["no rule applies", "no rule applies"]);

		// The audio-only clip loses its audio: a warning, not a skip. A
		// file with nothing left is skipped; the others still run.
		let audio = parse(
			r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"aac","tags":{"language":"fre"}}]}"#,
		)
		.unwrap();
		let b = Batch {
			streams: Streams::Rules(drop_all),
			..batch("{name}.mkv", into("/out"))
		};
		let items = plan(
			&b,
			&[
				input("/in/movie.mp4", movie()),
				input("/in/audio.mka", audio),
			],
			"",
		);
		assert_eq!(
			items[0].ready().unwrap().warnings,
			["the rules drop every video stream of the input"]
		);
		assert!(items[1].ready().is_some());

		let b = Batch {
			streams: Streams::Rules(Rules {
				rules: vec![Rule::drop(None, Matcher::Codec("aac".into()))],
				untagged: Untagged::Keep,
			}),
			..batch("{name}.mkv", into("/out"))
		};
		let only_aac =
			parse(r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"aac"}]}"#).unwrap();
		let items = plan(
			&b,
			&[
				input("/in/a.mka", only_aac),
				input("/in/movie.mp4", movie()),
			],
			"",
		);
		assert_eq!(
			items[0].skip(),
			Some(&Skip::Streams("no stream is left to copy".into()))
		);
		assert_eq!(items[0].output, None);
		assert!(items[1].ready().is_some());
	}

	#[test]
	fn a_ticked_index_a_file_lacks_skips_only_that_file() {
		let b = Batch {
			streams: Streams::Selection(Selection::keep([0, 2])),
			..batch("{name}.mkv", into("/out"))
		};
		let items = plan(
			&b,
			&[
				input("/in/movie.mp4", movie()),
				input("/in/clip.mp4", clip()),
			],
			"",
		);
		assert_eq!(
			items[0].ready().unwrap().plan.kept().collect::<Vec<_>>(),
			[0, 2]
		);
		assert_eq!(items[0].ready().unwrap().whys[1], "unticked");
		assert_eq!(
			items[1].skip(),
			Some(&Skip::Streams("the input has no stream 2".into()))
		);
	}

	#[test]
	fn a_file_that_cannot_be_probed_is_skipped() {
		let inputs = [
			Input {
				path: PathBuf::from("/in/broken.mp4"),
				probe: Err("ffprobe exited with status 1".into()),
			},
			input("/in/clip.mp4", clip()),
		];
		let items = plan(&batch("{name}.mkv", into("/out")), &inputs, "");
		assert_eq!(
			items[0].skip(),
			Some(&Skip::Probe("ffprobe exited with status 1".into()))
		);
		assert_eq!(
			items[0].skip().unwrap().to_string(),
			"the file could not be probed: ffprobe exited with status 1"
		);
		assert_eq!(items[0].command(), None);
		assert!(items[1].ready().is_some());
	}

	#[test]
	fn the_container_is_checked_per_file() {
		let items = plan(
			&batch("{name}.mp4", into("/out")),
			&[
				input("/in/movie.mkv", movie()),
				input("/in/clip.mkv", clip()),
			],
			"",
		);
		assert_eq!(
			items[0].skip(),
			Some(&Skip::CannotHold {
				container: "MP4".into(),
				problems:
					"MP4 cannot hold subrip subtitles (stream 3); drop stream 3 or choose MKV"
						.into(),
			})
		);
		assert_eq!(
			items[0].output.as_deref(),
			Some(Path::new("/out/movie.mp4"))
		);
		assert!(items[1].ready().is_some());

		let items = plan(
			&batch("{name}.avi", into("/out")),
			&[input("/in/clip.mkv", clip())],
			"",
		);
		assert_eq!(
			items[0].skip().unwrap().to_string(),
			"the extension of clip.avi picks no container; use one of .mkv, .mka, .mks, .mp4, .m4v, .m4a, .mov, .webm"
		);

		// `{name}.{ext}` keeps every file's own container.
		let items = plan(
			&batch("{name}.{ext}", into("/out")),
			&[input("/in/a.mp4", clip()), input("/in/b.mov", clip())],
			"",
		);
		assert_eq!(items[0].ready().unwrap().plan.container, Container::Mp4);
		assert_eq!(items[1].ready().unwrap().plan.container, Container::Mov);
	}

	#[test]
	fn name_collisions_keep_the_first_file() {
		let inputs = [
			input("/in/a.mp4", clip()),
			input("/in/sub/a.mkv", clip()),
			input("/in/b.mp4", clip()),
		];
		let items = plan(&batch("{name}.mkv", into("/out")), &inputs, "");
		assert!(items[0].ready().is_some());
		assert_eq!(
			items[1].skip(),
			Some(&Skip::Collision(PathBuf::from("/in/a.mp4")))
		);
		assert_eq!(
			items[1].skip().unwrap().to_string(),
			"another file of the batch has the same output name: /in/a.mp4"
		);
		assert!(items[2].ready().is_some());

		// A template without a placeholder collides on every file but the
		// first.
		let items = plan(&batch("fixed.mkv", into("/out")), &inputs, "");
		assert!(items[0].ready().is_some());
		assert!(
			items[1..]
				.iter()
				.all(|i| matches!(i.skip(), Some(Skip::Collision(_))))
		);

		// Next to the input, the same name in another folder is fine.
		let items = plan(&batch("{name}.mkv", Destination::NextToInput), &inputs, "");
		assert_eq!(outputs(&items), ["/in/a.mkv", "/in/sub/a.mkv", "/in/b.mkv"]);
		assert!(items[0].ready().is_some());
		assert_eq!(items[1].skip(), Some(&Skip::SameAsInput));
		assert!(items[2].ready().is_some());
	}

	#[test]
	fn an_input_of_the_batch_is_never_an_output() {
		let inputs = [input("/in/a.mp4", clip()), input("/in/a.mkv", clip())];
		let b = Batch {
			existing: Existing::Overwrite,
			..batch("{name}.mkv", Destination::NextToInput)
		};
		let items = plan(&b, &inputs, "");
		assert_eq!(
			items[0].skip(),
			Some(&Skip::IsInput(PathBuf::from("/in/a.mkv")))
		);
		assert_eq!(items[1].skip(), Some(&Skip::SameAsInput));
	}

	#[test]
	fn existing_outputs_are_skipped_unless_overwriting_is_allowed() {
		let dir = tempfile::tempdir().unwrap();
		let out = dir.path().join("out");
		fs::create_dir(&out).unwrap();
		fs::write(out.join("a.mkv"), b"old").unwrap();
		let inputs = [input("/in/a.mp4", clip()), input("/in/b.mp4", clip())];

		let items = plan(
			&batch("{name}.mkv", Destination::Folder(out.clone())),
			&inputs,
			"",
		);
		assert_eq!(items[0].skip(), Some(&Skip::Exists));
		assert_eq!(
			items[0].skip().unwrap().to_string(),
			"the output exists; allow overwriting to replace it"
		);
		assert_eq!(
			items[0].output.as_deref(),
			Some(out.join("a.mkv").as_path())
		);
		assert!(items[1].ready().is_some());
		assert_eq!(fs::read(out.join("a.mkv")).unwrap(), b"old");

		let b = Batch {
			existing: Existing::Overwrite,
			..batch("{name}.mkv", Destination::Folder(out.clone()))
		};
		let items = plan(&b, &inputs, "");
		let a = items[0].ready().unwrap();
		assert!(a.overwrites);
		assert_eq!(
			items[0].command().unwrap().to_string(),
			format!(
				"ffmpeg -y -i /in/a.mp4 -map 0 -c copy {}",
				out.join("a.mkv").display()
			)
		);
		// The one that does not exist yet runs without -y.
		assert!(!items[1].ready().unwrap().overwrites);
		assert!(!items[1].command().unwrap().to_string().contains("-y"));
	}

	#[test]
	fn a_name_that_is_not_a_file_name_is_skipped() {
		let items = plan(
			&batch("{lang}", into("/out")),
			&[Input {
				path: PathBuf::from("/in/x.mp4"),
				probe: Ok(parse(
					r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264"}]}"#,
				)
				.unwrap()),
			}],
			"",
		);
		// No audio: `{lang}` is `und`, which has no extension.
		assert!(matches!(items[0].skip(), Some(Skip::NoContainer { name, .. }) if name == "und"));

		let items = plan(
			&batch("{ext}", into("/out")),
			&[input("/in/noext", clip())],
			"",
		);
		assert_eq!(items[0].skip(), Some(&Skip::BadName(String::new())));
		assert_eq!(items[0].output, None);
	}

	#[test]
	fn languages_are_distinct_and_in_stream_order() {
		let probe = parse(
			r#"{"streams":[
				{"index":0,"codec_type":"audio","tags":{"language":"fre"}},
				{"index":1,"codec_type":"audio","tags":{"language":"eng"}},
				{"index":2,"codec_type":"audio","tags":{"language":"fre"}},
				{"index":3,"codec_type":"audio","tags":{"language":"und"}},
				{"index":4,"codec_type":"subtitle","tags":{"language":"ger"}}
			]}"#,
		)
		.unwrap();
		assert_eq!(languages(&probe, &[0, 1, 2, 3, 4]), "fre-eng");
		assert_eq!(languages(&probe, &[1, 3, 4]), "eng");
		assert_eq!(languages(&probe, &[3, 4]), "und");
	}
}
