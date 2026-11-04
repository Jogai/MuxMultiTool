//! Saved presets: a named combination of a target container, stream filter
//! rules and an output file name template, for the combinations that come
//! back again and again ("English only, drop DTS, MKV").
//!
//! A [`Preset`] is not a new kind of operation. It is a stored set of the
//! settings a stream copy or a batch takes by hand, and applying it only
//! builds those settings: [`Preset::batch`] is the [`Batch`] and
//! [`Preset::output_template`] the template a hand-entered batch with the
//! same rules would use, so a preset plans exactly like the settings typed
//! in (the tests compare the two plans).
//!
//! The [`template`](Preset::template) is the output name without the
//! extension, e.g. `{name}.{lang}`; the [`container`](Preset::container)
//! adds the extension that picks the container, so the two cannot disagree.
//!
//! # Storage
//!
//! The [`PresetStore`] keeps every preset in `presets.json` in the data
//! folder, written atomically on every change:
//!
//! ```json
//! { "version": 1, "next_id": 3, "presets": [ { "id": "1", "name": "...", ... } ] }
//! ```
//!
//! `version` is the format. A file of a version this build does not read
//! (a newer one, or none) is never overwritten and never guessed at: every
//! operation then fails with [`Error::Unreadable`], which says what is
//! wrong, until the file is fixed or removed.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::batch::{Batch, Destination, Existing, Streams, Template, TemplateError};
use crate::ffmpeg::probe::CodecType;
use crate::streams::Container;
use crate::streams::rules::{self, Action, Matcher, Rule, Rules, Untagged};

/// File name of the presets in the data folder.
pub const PRESETS_FILE: &str = "presets.json";
/// The format version this build writes and reads.
pub const FORMAT_VERSION: u32 = 1;
/// The longest preset name, in characters.
pub const NAME_LIMIT: usize = 100;

/// The settings a preset stores, without its identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
	/// The target container.
	pub container: Container,
	/// The output name without its extension; see the module docs.
	pub template: String,
	/// The stream filter rules.
	pub rules: Rules,
}

/// A saved preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
	/// Stable id, assigned when the preset is created.
	pub id: String,
	/// The name the user sees; unique, ignoring case.
	pub name: String,
	/// The settings.
	pub settings: Settings,
}

impl Settings {
	/// Checks the template and the rules.
	///
	/// # Errors
	/// [`Error::Invalid`] with the reason.
	pub fn validate(&self) -> Result<(), Error> {
		self.output_template()?;
		self.rules
			.validate()
			.map_err(|error| Error::Invalid(error.to_string()))
	}

	/// The full output template: the name template plus the container's
	/// extension. This is the string a hand-entered batch would use.
	#[must_use]
	pub fn output_template_source(&self) -> String {
		format!(
			"{}.{}",
			self.template,
			self.container
				.extensions()
				.first()
				.copied()
				.unwrap_or("mkv")
		)
	}

	/// [`output_template_source`](Self::output_template_source), parsed.
	///
	/// # Errors
	/// [`Error::Invalid`] when the template is empty, has a path separator
	/// or an unknown placeholder.
	pub fn output_template(&self) -> Result<Template, Error> {
		if self.template.trim().is_empty() {
			return Err(Error::Invalid(TemplateError::Empty.to_string()));
		}
		Template::parse(&self.output_template_source())
			.map_err(|error| Error::Invalid(error.to_string()))
	}

	/// The stream selection: the preset's rules.
	#[must_use]
	pub fn streams(&self) -> Streams {
		Streams::Rules(self.rules.clone())
	}

	/// The batch these settings make for the given destination and
	/// overwrite choice; the same value as building it by hand.
	///
	/// # Errors
	/// See [`output_template`](Self::output_template).
	pub fn batch(&self, destination: Destination, existing: Existing) -> Result<Batch, Error> {
		Ok(Batch {
			destination,
			template: self.output_template()?,
			existing,
			streams: self.streams(),
		})
	}
}

/// What can go wrong with the presets.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// No preset has this id.
	#[error("no preset with id {0}")]
	NotFound(String),
	/// Another preset has this name.
	#[error("a preset named {0:?} exists already")]
	DuplicateName(String),
	/// The name, template or rules are not valid.
	#[error("{0}")]
	Invalid(String),
	/// The presets file cannot be used; it is left as it is.
	#[error("{path} cannot be used: {reason}")]
	Unreadable {
		/// The file.
		path: PathBuf,
		/// What is wrong with it.
		reason: String,
	},
	/// The file or its folder could not be read or written.
	#[error("cannot access {path}: {source}")]
	Io {
		/// The file or folder.
		path: PathBuf,
		/// The underlying error.
		#[source]
		source: io::Error,
	},
}

#[derive(Debug)]
struct State {
	presets: Vec<Preset>,
	next_id: u64,
	/// Set when the file could not be read: the reason.
	unreadable: Option<String>,
}

/// Keeps the presets, in memory and optionally in a file.
#[derive(Debug)]
pub struct PresetStore {
	state: Mutex<State>,
	path: Option<PathBuf>,
}

impl PresetStore {
	/// A store that forgets everything when the process ends.
	#[must_use]
	pub fn in_memory() -> Self {
		Self::new(None, State::empty())
	}

	/// A store backed by `presets.json` in `data_dir`. A file that cannot
	/// be used does not stop the server: the store reports it on every
	/// call (see [`Error::Unreadable`]) and leaves the file alone.
	#[must_use]
	pub fn in_dir(data_dir: &Path) -> Self {
		let path = data_dir.join(PRESETS_FILE);
		let state = match std::fs::read(&path) {
			Ok(bytes) => match parse(&bytes) {
				Ok(state) => state,
				Err(reason) => {
					warn!(path = %path.display(), %reason, "presets file unusable");
					State {
						unreadable: Some(reason),
						..State::empty()
					}
				}
			},
			Err(source) if source.kind() == io::ErrorKind::NotFound => State::empty(),
			Err(source) => State {
				unreadable: Some(source.to_string()),
				..State::empty()
			},
		};
		Self::new(Some(path), state)
	}

	/// [`Self::in_dir`] when a folder is configured, else
	/// [`Self::in_memory`].
	#[must_use]
	pub fn from_config(data_dir: Option<&Path>) -> Self {
		let Some(dir) = data_dir else {
			info!("no data folder configured; presets are kept in memory only");
			return Self::in_memory();
		};
		Self::in_dir(dir)
	}

	const fn new(path: Option<PathBuf>, state: State) -> Self {
		Self {
			state: Mutex::const_new(state),
			path,
		}
	}

	/// Where the presets are written, if anywhere.
	#[must_use]
	pub fn path(&self) -> Option<&Path> {
		self.path.as_deref()
	}

	fn unreadable(&self, reason: &str) -> Error {
		Error::Unreadable {
			path: self.path.clone().unwrap_or_default(),
			reason: reason.to_owned(),
		}
	}

	/// Every preset, in creation order.
	///
	/// # Errors
	/// [`Error::Unreadable`] when the stored file could not be used.
	pub async fn list(&self) -> Result<Vec<Preset>, Error> {
		let state = self.state.lock().await;
		match &state.unreadable {
			Some(reason) => Err(self.unreadable(reason)),
			None => Ok(state.presets.clone()),
		}
	}

	/// One preset.
	///
	/// # Errors
	/// [`Error::NotFound`], or see [`list`](Self::list).
	pub async fn get(&self, id: &str) -> Result<Preset, Error> {
		self.list()
			.await?
			.into_iter()
			.find(|p| p.id == id)
			.ok_or_else(|| Error::NotFound(id.to_owned()))
	}

	/// Saves a new preset.
	///
	/// # Errors
	/// [`Error::Invalid`], [`Error::DuplicateName`], [`Error::Io`], or see
	/// [`list`](Self::list).
	pub async fn create(&self, name: &str, settings: Settings) -> Result<Preset, Error> {
		let name = checked_name(name)?;
		settings.validate()?;
		self.change(|state| {
			check_unique(&state.presets, &name, None)?;
			let preset = Preset {
				id: state.next_id.to_string(),
				name,
				settings,
			};
			state.next_id += 1;
			state.presets.push(preset.clone());
			Ok(preset)
		})
		.await
	}

	/// Replaces a preset's settings, keeping its name.
	///
	/// # Errors
	/// [`Error::NotFound`], [`Error::Invalid`], [`Error::Io`], or see
	/// [`list`](Self::list).
	pub async fn update(&self, id: &str, settings: Settings) -> Result<Preset, Error> {
		settings.validate()?;
		self.change(|state| {
			let preset = find(&mut state.presets, id)?;
			preset.settings = settings;
			Ok(preset.clone())
		})
		.await
	}

	/// Changes a preset's name.
	///
	/// # Errors
	/// [`Error::NotFound`], [`Error::Invalid`], [`Error::DuplicateName`],
	/// [`Error::Io`], or see [`list`](Self::list).
	pub async fn rename(&self, id: &str, name: &str) -> Result<Preset, Error> {
		let name = checked_name(name)?;
		self.change(|state| {
			check_unique(&state.presets, &name, Some(id))?;
			let preset = find(&mut state.presets, id)?;
			preset.name = name;
			Ok(preset.clone())
		})
		.await
	}

	/// Deletes a preset.
	///
	/// # Errors
	/// [`Error::NotFound`], [`Error::Io`], or see [`list`](Self::list).
	pub async fn delete(&self, id: &str) -> Result<(), Error> {
		self.change(|state| {
			let at = state
				.presets
				.iter()
				.position(|p| p.id == id)
				.ok_or_else(|| Error::NotFound(id.to_owned()))?;
			state.presets.remove(at);
			Ok(())
		})
		.await
	}

	/// Applies `edit` to a copy of the state and stores the result; the
	/// state changes only when the write succeeded.
	async fn change<T>(
		&self,
		edit: impl FnOnce(&mut State) -> Result<T, Error>,
	) -> Result<T, Error> {
		let mut state = self.state.lock().await;
		if let Some(reason) = &state.unreadable {
			return Err(self.unreadable(reason));
		}
		let mut next = State {
			presets: state.presets.clone(),
			next_id: state.next_id,
			unreadable: None,
		};
		let result = edit(&mut next)?;
		if let Some(path) = &self.path {
			write_atomically(path, &next).await?;
		}
		*state = next;
		Ok(result)
	}
}

impl State {
	const fn empty() -> Self {
		Self {
			presets: Vec::new(),
			next_id: 1,
			unreadable: None,
		}
	}
}

fn find<'a>(presets: &'a mut [Preset], id: &str) -> Result<&'a mut Preset, Error> {
	presets
		.iter_mut()
		.find(|p| p.id == id)
		.ok_or_else(|| Error::NotFound(id.to_owned()))
}

fn checked_name(name: &str) -> Result<String, Error> {
	let name = name.trim();
	if name.is_empty() {
		return Err(Error::Invalid("the preset name is empty".into()));
	}
	if name.chars().count() > NAME_LIMIT {
		return Err(Error::Invalid(format!(
			"the preset name is longer than {NAME_LIMIT} characters"
		)));
	}
	Ok(name.to_owned())
}

fn check_unique(presets: &[Preset], name: &str, except: Option<&str>) -> Result<(), Error> {
	if presets
		.iter()
		.any(|p| Some(p.id.as_str()) != except && p.name.to_lowercase() == name.to_lowercase())
	{
		return Err(Error::DuplicateName(name.to_owned()));
	}
	Ok(())
}

// ---- stored format ----

#[derive(Debug, Serialize, Deserialize)]
struct StoredFile {
	version: u32,
	next_id: u64,
	presets: Vec<StoredPreset>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredPreset {
	id: String,
	name: String,
	/// The container's usual extension: `mkv`, `mp4`, `mov` or `webm`.
	container: String,
	template: String,
	rules: StoredRules,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredRules {
	/// `keep` or `drop`.
	untagged: String,
	rules: Vec<StoredRule>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredRule {
	/// `keep` or `drop`.
	action: String,
	/// `video`, `audio`, `subtitle`, `data` or `attachment`; absent for
	/// every type.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	stream_type: Option<String>,
	#[serde(rename = "match")]
	matcher: StoredMatch,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredMatch {
	Language(String),
	Codec(String),
	Stream(u32),
}

fn stream_type_name(t: CodecType) -> Option<&'static str> {
	match t {
		CodecType::Video => Some("video"),
		CodecType::Audio => Some("audio"),
		CodecType::Subtitle => Some("subtitle"),
		CodecType::Data => Some("data"),
		CodecType::Attachment => Some("attachment"),
		CodecType::Unknown => None,
	}
}

fn stream_type_from(name: &str) -> Result<CodecType, String> {
	Ok(match name {
		"video" => CodecType::Video,
		"audio" => CodecType::Audio,
		"subtitle" => CodecType::Subtitle,
		"data" => CodecType::Data,
		"attachment" => CodecType::Attachment,
		other => return Err(format!("unknown stream type {other:?}")),
	})
}

impl From<&Preset> for StoredPreset {
	fn from(preset: &Preset) -> Self {
		let rules = &preset.settings.rules;
		Self {
			id: preset.id.clone(),
			name: preset.name.clone(),
			container: preset
				.settings
				.container
				.extensions()
				.first()
				.copied()
				.unwrap_or("mkv")
				.to_owned(),
			template: preset.settings.template.clone(),
			rules: StoredRules {
				untagged: match rules.untagged {
					Untagged::Keep => "keep",
					Untagged::Drop => "drop",
				}
				.to_owned(),
				rules: rules
					.rules
					.iter()
					.map(|r| StoredRule {
						action: r.action.to_string(),
						stream_type: r.stream_type.and_then(stream_type_name).map(str::to_owned),
						matcher: match &r.matcher {
							Matcher::Language(v) => StoredMatch::Language(v.clone()),
							Matcher::Codec(v) => StoredMatch::Codec(v.clone()),
							Matcher::Stream(i) => StoredMatch::Stream(*i),
						},
					})
					.collect(),
			},
		}
	}
}

impl TryFrom<StoredPreset> for Preset {
	type Error = String;

	fn try_from(stored: StoredPreset) -> Result<Self, String> {
		let container = Container::from_extension(Path::new(&format!("x.{}", stored.container)))
			.ok_or_else(|| {
				format!(
					"preset {:?}: unknown container {:?}",
					stored.name, stored.container
				)
			})?;
		let untagged = match stored.rules.untagged.as_str() {
			"keep" => Untagged::Keep,
			"drop" => Untagged::Drop,
			other => {
				return Err(format!(
					"preset {:?}: unknown untagged {other:?}",
					stored.name
				));
			}
		};
		let rules = stored
			.rules
			.rules
			.into_iter()
			.map(|r| {
				let stream_type = r.stream_type.as_deref().map(stream_type_from).transpose()?;
				let matcher = match r.matcher {
					StoredMatch::Language(v) => Matcher::Language(v),
					StoredMatch::Codec(v) => Matcher::Codec(v),
					StoredMatch::Stream(i) => Matcher::Stream(i),
				};
				let action = match r.action.as_str() {
					"drop" => Action::Drop,
					"keep" => Action::Keep,
					other => return Err(format!("unknown action {other:?}")),
				};
				Ok(Rule {
					action,
					stream_type,
					matcher,
				})
			})
			.collect::<Result<Vec<_>, String>>()
			.map_err(|e| format!("preset {:?}: {e}", stored.name))?;
		let preset = Self {
			id: stored.id,
			name: stored.name,
			settings: Settings {
				container,
				template: stored.template,
				rules: Rules { rules, untagged },
			},
		};
		preset
			.settings
			.validate()
			.map_err(|e| format!("preset {:?}: {e}", preset.name))?;
		Ok(preset)
	}
}

/// Reads the file's bytes, dispatching on the format version. A new
/// version adds an arm here that converts the old layout.
fn parse(bytes: &[u8]) -> Result<State, String> {
	let value: serde_json::Value =
		serde_json::from_slice(bytes).map_err(|e| format!("not valid JSON: {e}"))?;
	let version = value
		.get("version")
		.and_then(serde_json::Value::as_u64)
		.ok_or("it has no format version")?;
	match version {
		1 => {
			let file: StoredFile = serde_json::from_value(value)
				.map_err(|e| format!("not a valid presets file: {e}"))?;
			let presets = file
				.presets
				.into_iter()
				.map(Preset::try_from)
				.collect::<Result<Vec<_>, _>>()?;
			let next_id = presets
				.iter()
				.filter_map(|p| p.id.parse::<u64>().ok())
				.max()
				.map_or(1, |m| m + 1)
				.max(file.next_id);
			Ok(State {
				presets,
				next_id,
				unreadable: None,
			})
		}
		newer if newer > u64::from(FORMAT_VERSION) => Err(format!(
			"it is format version {newer}, written by a newer version of this program; \
			 this one reads up to version {FORMAT_VERSION}"
		)),
		other => Err(format!("unknown format version {other}")),
	}
}

/// Serialises `state` and writes it with [`crate::fs::write_atomically`],
/// so a crash mid-write leaves the old file intact.
async fn write_atomically(path: &Path, state: &State) -> Result<(), Error> {
	let io = |source| Error::Io {
		path: path.to_path_buf(),
		source,
	};
	let file = StoredFile {
		version: FORMAT_VERSION,
		next_id: state.next_id,
		presets: state.presets.iter().map(StoredPreset::from).collect(),
	};
	let json = serde_json::to_vec_pretty(&file).map_err(|e| io(io::Error::other(e)))?;
	crate::fs::write_atomically(path, json).await.map_err(io)
}

/// Resolution errors of the rules, for callers that apply a preset.
pub use rules::Error as RulesError;

#[cfg(test)]
mod tests {
	use super::*;
	use crate::batch::{self, Input};

	/// The names of the entries in `dir`, sorted.
	fn entries(dir: &Path) -> Vec<String> {
		let mut names: Vec<String> = std::fs::read_dir(dir)
			.unwrap()
			.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
			.collect();
		names.sort();
		names
	}
	use crate::ffmpeg::probe::parse as parse_probe;
	use crate::streams::rules::plan as rules_plan;

	fn english_only() -> Settings {
		Settings {
			container: Container::Mkv,
			template: "{name}.{lang}".into(),
			rules: Rules {
				rules: vec![
					Rule::keep(Some(CodecType::Audio), Matcher::Language("eng".into())),
					Rule::drop(None, Matcher::Codec("dts".into())),
				],
				untagged: Untagged::Drop,
			},
		}
	}

	fn movie() -> crate::ffmpeg::ProbeResult {
		parse_probe(
			r#"{"format":{"duration":"120.5"},"streams":[
				{"index":0,"codec_type":"video","codec_name":"h264"},
				{"index":1,"codec_type":"audio","codec_name":"aac","tags":{"language":"eng"}},
				{"index":2,"codec_type":"audio","codec_name":"dts","tags":{"language":"fre"}},
				{"index":3,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"}}
			]}"#,
		)
		.unwrap()
	}

	#[tokio::test]
	async fn presets_are_created_listed_renamed_updated_and_deleted() {
		let store = PresetStore::in_memory();
		assert_eq!(store.list().await.unwrap(), Vec::new());
		let first = store
			.create(" English only ", english_only())
			.await
			.unwrap();
		assert_eq!(first.name, "English only");
		let mut other = english_only();
		other.container = Container::Mp4;
		let second = store.create("MP4", other.clone()).await.unwrap();
		assert_ne!(first.id, second.id);
		assert_eq!(
			store.list().await.unwrap(),
			vec![first.clone(), second.clone()]
		);

		let renamed = store.rename(&first.id, "EN").await.unwrap();
		assert_eq!(renamed.name, "EN");
		let mut changed = english_only();
		changed.template = "{name}".into();
		let updated = store.update(&first.id, changed.clone()).await.unwrap();
		assert_eq!((updated.name.as_str(), updated.settings), ("EN", changed));
		assert_eq!(store.get(&second.id).await.unwrap().settings, other);

		store.delete(&first.id).await.unwrap();
		assert!(matches!(
			store.get(&first.id).await,
			Err(Error::NotFound(_))
		));
		assert!(matches!(
			store.delete(&first.id).await,
			Err(Error::NotFound(_))
		));
		// An id is not reused after its preset is deleted.
		let third = store.create("again", english_only()).await.unwrap();
		assert_ne!(third.id, first.id);
		assert_ne!(third.id, second.id);
	}

	#[tokio::test]
	async fn names_are_checked() {
		let store = PresetStore::in_memory();
		let a = store.create("Movies", english_only()).await.unwrap();
		assert!(matches!(
			store.create("movies", english_only()).await,
			Err(Error::DuplicateName(_))
		));
		assert!(matches!(
			store.create("  ", english_only()).await,
			Err(Error::Invalid(_))
		));
		let long = "x".repeat(NAME_LIMIT + 1);
		assert!(matches!(
			store.create(&long, english_only()).await,
			Err(Error::Invalid(_))
		));
		let b = store.create("Other", english_only()).await.unwrap();
		assert!(matches!(
			store.rename(&b.id, "MOVIES").await,
			Err(Error::DuplicateName(_))
		));
		// Renaming to its own name in another case is fine.
		store.rename(&a.id, "MOVIES").await.unwrap();
	}

	#[tokio::test]
	async fn settings_are_validated() {
		let store = PresetStore::in_memory();
		for template in ["", "a/b", "{nope}", "{name"] {
			let mut settings = english_only();
			settings.template = template.into();
			assert!(
				matches!(store.create("x", settings).await, Err(Error::Invalid(_))),
				"{template}"
			);
		}
		let mut settings = english_only();
		settings
			.rules
			.rules
			.push(Rule::drop(None, Matcher::Language(" ".into())));
		assert!(matches!(
			store.create("x", settings).await,
			Err(Error::Invalid(_))
		));
		assert_eq!(store.list().await.unwrap(), Vec::new());
	}

	#[tokio::test]
	async fn presets_survive_a_restart() {
		let dir = tempfile::tempdir().unwrap();
		let data = dir.path().join("nested/data");
		let store = PresetStore::in_dir(&data);
		let mut settings = english_only();
		settings
			.rules
			.rules
			.push(Rule::drop(None, Matcher::Stream(3)));
		let saved = store.create("English only", settings).await.unwrap();
		assert!(data.join(PRESETS_FILE).is_file());
		assert_eq!(
			entries(&data),
			[PRESETS_FILE],
			"no temp file is left behind"
		);

		let reopened = PresetStore::in_dir(&data);
		assert_eq!(reopened.list().await.unwrap(), vec![saved]);
		let next = reopened.create("next", english_only()).await.unwrap();
		assert_eq!(next.id, "2");
	}

	#[tokio::test]
	async fn the_stored_format_is_versioned_and_stable() {
		let dir = tempfile::tempdir().unwrap();
		let store = PresetStore::in_dir(dir.path());
		store.create("English only", english_only()).await.unwrap();
		let json: serde_json::Value =
			serde_json::from_slice(&std::fs::read(dir.path().join(PRESETS_FILE)).unwrap()).unwrap();
		assert_eq!(
			json,
			serde_json::json!({
				"version": 1,
				"next_id": 2,
				"presets": [{
					"id": "1",
					"name": "English only",
					"container": "mkv",
					"template": "{name}.{lang}",
					"rules": {
						"untagged": "drop",
						"rules": [
							{"action": "keep", "stream_type": "audio", "match": {"language": "eng"}},
							{"action": "drop", "match": {"codec": "dts"}}
						]
					}
				}]
			})
		);
	}

	#[tokio::test]
	async fn a_file_of_another_version_is_reported_and_left_alone() {
		for (content, expect) in [
			(r#"{"version":2,"next_id":1,"presets":[]}"#, "newer version"),
			(r#"{"presets":[]}"#, "no format version"),
			("not json", "not valid JSON"),
			(
				r#"{"version":1,"next_id":2,"presets":[{"id":"1","name":"x","container":"avi","template":"{name}","rules":{"untagged":"keep","rules":[]}}]}"#,
				"unknown container",
			),
		] {
			let dir = tempfile::tempdir().unwrap();
			let path = dir.path().join(PRESETS_FILE);
			std::fs::write(&path, content).unwrap();
			let store = PresetStore::in_dir(dir.path());
			let error = store.list().await.unwrap_err();
			assert!(matches!(error, Error::Unreadable { .. }));
			assert!(error.to_string().contains(expect), "{error}");
			assert!(matches!(
				store.create("x", english_only()).await,
				Err(Error::Unreadable { .. })
			));
			assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
		}
	}

	#[tokio::test]
	async fn a_failed_write_changes_nothing() {
		let dir = tempfile::tempdir().unwrap();
		let store = PresetStore::in_dir(dir.path());
		// A folder where the file goes: the rename over it fails.
		std::fs::create_dir(dir.path().join(PRESETS_FILE)).unwrap();
		assert!(matches!(
			store.create("x", english_only()).await,
			Err(Error::Io { .. })
		));
		assert_eq!(store.list().await.unwrap(), Vec::new());
	}

	#[test]
	fn the_container_adds_the_extension() {
		let mut settings = english_only();
		assert_eq!(settings.output_template_source(), "{name}.{lang}.mkv");
		settings.container = Container::Mp4;
		assert_eq!(
			settings.output_template().unwrap().source(),
			"{name}.{lang}.mp4"
		);
	}

	/// The batch a preset makes is the batch typed in by hand, and plans
	/// the same.
	#[test]
	fn applying_a_preset_to_a_batch_equals_the_hand_entered_settings() {
		let settings = english_only();
		let destination = Destination::Folder(PathBuf::from("/out"));
		let from_preset = settings.batch(destination.clone(), Existing::Skip).unwrap();
		let by_hand = Batch {
			destination,
			template: Template::parse("{name}.{lang}.mkv").unwrap(),
			existing: Existing::Skip,
			streams: Streams::Rules(Rules {
				rules: vec![
					Rule::keep(Some(CodecType::Audio), Matcher::Language("eng".into())),
					Rule::drop(None, Matcher::Codec("dts".into())),
				],
				untagged: Untagged::Drop,
			}),
		};
		assert_eq!(from_preset, by_hand);

		let inputs = [
			Input {
				path: PathBuf::from("/in/a.mp4"),
				probe: Ok(movie()),
			},
			Input {
				path: PathBuf::from("/in/b.mp4"),
				probe: Ok(movie()),
			},
		];
		let planned = batch::plan(&from_preset, &inputs, "2026-11-04");
		assert_eq!(planned, batch::plan(&by_hand, &inputs, "2026-11-04"));
		let first = planned[0].ready().unwrap();
		assert_eq!(planned[0].output, Some(PathBuf::from("/out/a.eng.mkv")));
		assert_eq!(first.plan.kept().collect::<Vec<_>>(), vec![0, 1, 3]);
	}

	/// For a single file the stream plan of the preset's rules and
	/// container is the plan of the same settings entered by hand.
	#[test]
	fn applying_a_preset_to_a_file_equals_the_hand_entered_settings() {
		let settings = english_only();
		let probe = movie();
		let from_preset = rules_plan(&probe, &settings.rules, settings.container).unwrap();
		let by_hand = rules_plan(
			&probe,
			&Rules {
				rules: vec![
					Rule::keep(Some(CodecType::Audio), Matcher::Language("eng".into())),
					Rule::drop(None, Matcher::Codec("dts".into())),
				],
				untagged: Untagged::Drop,
			},
			Container::from_extension(Path::new("out.mkv")).unwrap(),
		)
		.unwrap();
		assert_eq!(from_preset.plan, by_hand.plan);
		assert_eq!(from_preset.decisions, by_hand.decisions);
		// The batch, with this one file, plans the same streams.
		let inputs = [Input {
			path: PathBuf::from("/in/a.mp4"),
			probe: Ok(probe),
		}];
		let batch = settings
			.batch(Destination::NextToInput, Existing::Overwrite)
			.unwrap();
		let item = &batch::plan(&batch, &inputs, "2026-11-04")[0];
		assert_eq!(item.ready().unwrap().plan, from_preset.plan);
	}
}
