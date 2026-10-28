//! What the installed ffmpeg build can do.
//!
//! Builds differ per platform and licence: one lacks `libx265`, another has
//! no `NVENC`. [`detect`] asks the build, through `ffmpeg -version`,
//! `-encoders`, `-decoders`, `-formats`, `-filters`, `-hwaccels` and `-bsfs`,
//! and the `parse_*` functions turn that text into [`Capabilities`]. The
//! frontend uses the result to grey out options instead of letting a job
//! fail.
//!
//! Detection costs a handful of process starts, so [`CapabilityCache`] keeps
//! the result and only repeats it when `ffmpeg -version` reports something
//! else, for example after the binary was replaced.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use super::runner::{Limits, RunError, Runner};
use super::tools::Tool;

/// How long one capability query may take. They normally finish in
/// milliseconds; this only stops a broken binary from hanging detection.
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// What kind of media a codec handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecKind {
	/// Video.
	Video,
	/// Audio.
	Audio,
	/// Subtitles.
	Subtitle,
	/// Anything else, such as data streams.
	Other,
}

/// An encoder or decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Codec {
	/// The name to pass to `-c:v` and friends, e.g. `libx264`.
	pub name: String,
	/// What kind of media it handles.
	pub kind: CodecKind,
	/// Human-readable description.
	pub description: String,
}

/// A container format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
	/// The name to pass to `-f`, e.g. `matroska`.
	pub name: String,
	/// Human-readable description.
	pub description: String,
	/// Whether ffmpeg can read it.
	pub demux: bool,
	/// Whether ffmpeg can write it.
	pub mux: bool,
}

/// A filter usable in `-vf`, `-af` and `-filter_complex`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
	/// The filter name, e.g. `scale`.
	pub name: String,
	/// Human-readable description.
	pub description: String,
	/// Whether it supports timeline editing (the `enable` option).
	pub timeline: bool,
}

/// Everything the build reports about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
	/// The version string, e.g. `7.1.1` or `N-128122-g215351c1c3-20261027`.
	pub version: String,
	/// Encoders, which decide what a job can write.
	pub encoders: Vec<Codec>,
	/// Decoders, which decide what a job can read.
	pub decoders: Vec<Codec>,
	/// Container formats. Devices (cameras, screen capture) are left out.
	pub formats: Vec<Format>,
	/// Filters.
	pub filters: Vec<Filter>,
	/// Hardware acceleration methods, e.g. `vaapi`.
	pub hwaccels: Vec<String>,
	/// Bitstream filters (`-bsf`), e.g. `h264_metadata`, which edit packets
	/// without decoding them.
	pub bitstream_filters: Vec<String>,
}

impl Capabilities {
	/// Whether an encoder with this exact name exists.
	#[must_use]
	pub fn has_encoder(&self, name: &str) -> bool {
		self.encoders.iter().any(|c| c.name == name)
	}

	/// Whether a filter with this exact name exists.
	#[must_use]
	pub fn has_filter(&self, name: &str) -> bool {
		self.filters.iter().any(|f| f.name == name)
	}

	/// Whether a bitstream filter with this exact name exists.
	#[must_use]
	pub fn has_bitstream_filter(&self, name: &str) -> bool {
		self.bitstream_filters.iter().any(|f| f == name)
	}
}

/// Why capabilities are not available.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
	/// ffmpeg could not be started or exited with an error.
	#[error(transparent)]
	Run(#[from] RunError),
	/// ffmpeg ran, but its version line was not in the known shape.
	#[error("cannot read the ffmpeg version from its output")]
	UnknownVersion,
}

/// Parses `ffmpeg -version`: the word after `version` on the first line.
#[must_use]
pub fn parse_version(text: &str) -> Option<String> {
	let first = text.lines().next()?;
	let mut words = first.split_whitespace();
	words.find(|w| *w == "version")?;
	words.next().map(str::to_owned)
}

/// Parses `ffmpeg -encoders` or `-decoders`.
#[must_use]
pub fn parse_codecs(text: &str) -> Vec<Codec> {
	table_rows(text)
		.filter_map(|row| {
			let mut parts = row.split_whitespace();
			let flags = parts.next()?;
			let name = parts.next()?;
			let kind = match flags.chars().next()? {
				'V' => CodecKind::Video,
				'A' => CodecKind::Audio,
				'S' => CodecKind::Subtitle,
				_ => CodecKind::Other,
			};
			Some(Codec {
				name: name.to_owned(),
				kind,
				description: description_after(row, name),
			})
		})
		.collect()
}

/// Parses `ffmpeg -formats`. A line such as `matroska,webm` is one demuxer
/// that answers to several names; each name becomes its own entry. A name
/// that appears on several lines (`mp4` is both a demuxer alias and a muxer)
/// becomes one entry that can do both.
#[must_use]
pub fn parse_formats(text: &str) -> Vec<Format> {
	let mut formats: Vec<Format> = Vec::new();
	let lines = table_rows(text)
		.filter_map(|row| {
			// A space, then three fixed-width flags: ` DE  name`, ` D d name`.
			let flags: Vec<char> = row.chars().skip(1).take(3).collect();
			let [demux, mux, device] = flags[..] else {
				return None;
			};
			if device == 'd' {
				return None;
			}
			let rest = row.get(4..)?.trim_start();
			let names = rest.split_whitespace().next()?;
			Some((
				demux == 'D',
				mux == 'E',
				names,
				description_after(rest, names),
			))
		})
		.flat_map(|(demux, mux, names, description)| {
			names.split(',').map(move |name| Format {
				name: name.to_owned(),
				description: description.clone(),
				demux,
				mux,
			})
		});
	for format in lines {
		if let Some(known) = formats.iter_mut().find(|f| f.name == format.name) {
			known.demux |= format.demux;
			known.mux |= format.mux;
		} else {
			formats.push(format);
		}
	}
	formats
}

/// Parses `ffmpeg -filters`.
#[must_use]
pub fn parse_filters(text: &str) -> Vec<Filter> {
	table_rows(text)
		.filter_map(|row| {
			let mut parts = row.split_whitespace();
			let flags = parts.next()?;
			let name = parts.next()?;
			// The third column is the input/output shape, e.g. `V->V`.
			let shape = parts.next()?;
			if !shape.contains("->") {
				return None;
			}
			Some(Filter {
				name: name.to_owned(),
				description: description_after(row, shape),
				timeline: flags.starts_with('T'),
			})
		})
		.collect()
}

/// Parses `ffmpeg -hwaccels`: a header line, then one method per line.
#[must_use]
pub fn parse_hwaccels(text: &str) -> Vec<String> {
	text.lines()
		.skip_while(|l| !l.starts_with("Hardware acceleration methods"))
		.skip(1)
		.map(str::trim)
		.filter(|l| !l.is_empty())
		.map(str::to_owned)
		.collect()
}

/// Parses `ffmpeg -bsfs`: a header line, then one bitstream filter per line.
#[must_use]
pub fn parse_bitstream_filters(text: &str) -> Vec<String> {
	text.lines()
		.skip_while(|l| !l.starts_with("Bitstream filters"))
		.skip(1)
		.map(str::trim)
		.filter(|l| !l.is_empty())
		.map(str::to_owned)
		.collect()
}

/// The lines after the `------` that ends a table's legend. Empty when the
/// separator is missing, so unexpected output yields nothing instead of junk.
fn table_rows(text: &str) -> impl Iterator<Item = &str> {
	text.lines()
		.skip_while(|l| !l.trim().chars().all(|c| c == '-') || l.trim().is_empty())
		.skip(1)
}

/// The text after the first occurrence of `word` in `line`, trimmed.
fn description_after(line: &str, word: &str) -> String {
	line.find(word)
		.map(|at| line[at + word.len()..].trim().to_owned())
		.unwrap_or_default()
}

/// Runs the queries and parses the results.
///
/// # Errors
/// See [`CapabilityError`]. Any single failing query fails the whole
/// detection, because a half-known build would hide features wrongly.
pub async fn detect(runner: &Runner) -> Result<Capabilities, CapabilityError> {
	let version = read_version(runner).await?;
	detect_with_version(runner, version).await
}

async fn read_version(runner: &Runner) -> Result<String, CapabilityError> {
	let text = query(runner, "-version").await?;
	parse_version(&text).ok_or(CapabilityError::UnknownVersion)
}

async fn detect_with_version(
	runner: &Runner,
	version: String,
) -> Result<Capabilities, CapabilityError> {
	let (encoders, decoders, formats, filters, hwaccels, bsfs) = tokio::try_join!(
		query(runner, "-encoders"),
		query(runner, "-decoders"),
		query(runner, "-formats"),
		query(runner, "-filters"),
		query(runner, "-hwaccels"),
		query(runner, "-bsfs"),
	)?;
	Ok(Capabilities {
		version,
		encoders: parse_codecs(&encoders),
		decoders: parse_codecs(&decoders),
		formats: parse_formats(&formats),
		filters: parse_filters(&filters),
		hwaccels: parse_hwaccels(&hwaccels),
		bitstream_filters: parse_bitstream_filters(&bsfs),
	})
}

async fn query(runner: &Runner, flag: &str) -> Result<String, RunError> {
	runner
		.capture(
			Tool::Ffmpeg,
			&["-hide_banner", flag],
			&Limits::timeout(QUERY_TIMEOUT),
		)
		.await
}

/// Keeps the detected [`Capabilities`] until ffmpeg's version changes.
///
/// A failure is never cached, so installing ffmpeg while the server runs
/// fixes the next request without a restart.
#[derive(Debug, Default)]
pub struct CapabilityCache {
	cached: Mutex<Option<Arc<Capabilities>>>,
}

impl CapabilityCache {
	/// An empty cache.
	#[must_use]
	pub fn new() -> Self {
		Self::default()
	}

	/// The capabilities of the build at `runner`'s ffmpeg path.
	///
	/// Always asks `ffmpeg -version` (cheap) and compares it with the cached
	/// value; only a different version triggers the full detection.
	///
	/// # Errors
	/// See [`CapabilityError`]. A stale cache entry is dropped when ffmpeg
	/// stops working.
	pub async fn get(&self, runner: &Runner) -> Result<Arc<Capabilities>, CapabilityError> {
		// Held across the queries on purpose: concurrent callers wait for one
		// detection instead of each starting their own.
		let mut cached = self.cached.lock().await;
		let version = match read_version(runner).await {
			Ok(version) => version,
			Err(error) => {
				*cached = None;
				return Err(error);
			}
		};
		if let Some(hit) = cached.as_ref().filter(|c| c.version == version) {
			return Ok(Arc::clone(hit));
		}
		match detect_with_version(runner, version).await {
			Ok(found) => {
				let found = Arc::new(found);
				*cached = Some(Arc::clone(&found));
				Ok(found)
			}
			Err(error) => {
				*cached = None;
				Err(error)
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// Output recorded from a real ffmpeg build, trimmed to a few lines per list.
	const VERSION: &str = include_str!("../../tests/fixtures/ffmpeg/version.txt");
	const ENCODERS: &str = include_str!("../../tests/fixtures/ffmpeg/encoders.txt");
	const DECODERS: &str = include_str!("../../tests/fixtures/ffmpeg/decoders.txt");
	const FORMATS: &str = include_str!("../../tests/fixtures/ffmpeg/formats.txt");
	const FILTERS: &str = include_str!("../../tests/fixtures/ffmpeg/filters.txt");
	const HWACCELS: &str = include_str!("../../tests/fixtures/ffmpeg/hwaccels.txt");
	const BSFS: &str = include_str!("../../tests/fixtures/ffmpeg/bsfs.txt");

	#[test]
	fn version_is_the_word_after_version() {
		assert_eq!(
			parse_version(VERSION).as_deref(),
			Some("N-128122-g215351c1c3-20261027")
		);
		assert_eq!(
			parse_version("ffmpeg version 7.1.1-1ubuntu1 Copyright (c) 2000-2025\n").as_deref(),
			Some("7.1.1-1ubuntu1")
		);
	}

	#[test]
	fn version_of_something_else_is_none() {
		assert_eq!(parse_version(""), None);
		assert_eq!(parse_version("bash: ffmpeg: command not found"), None);
	}

	#[test]
	fn encoders_keep_name_kind_and_description() {
		let encoders = parse_codecs(ENCODERS);
		let x264 = encoders.iter().find(|c| c.name == "libx264").unwrap();
		assert_eq!(x264.kind, CodecKind::Video);
		assert!(x264.description.starts_with("libx264 H.264"));
		let aac = encoders.iter().find(|c| c.name == "aac").unwrap();
		assert_eq!(aac.kind, CodecKind::Audio);
		let srt = encoders.iter().find(|c| c.name == "srt").unwrap();
		assert_eq!(srt.kind, CodecKind::Subtitle);
		// The legend is not parsed as codecs
		assert!(encoders.iter().all(|c| c.name != "=" && c.name != "------"));
	}

	#[test]
	fn decoders_are_parsed_like_encoders() {
		let decoders = parse_codecs(DECODERS);
		assert!(decoders.iter().any(|c| c.name == "h264"));
		assert!(decoders.iter().any(|c| c.name == "opus"));
	}

	#[test]
	fn formats_split_aliases_and_skip_devices() {
		let formats = parse_formats(FORMATS);
		let by = |name: &str| formats.iter().find(|f| f.name == name).unwrap();
		// A muxer line and a `mov,mp4,…` demuxer line merge
		assert!(by("mp4").mux && by("mp4").demux);
		assert_eq!(formats.iter().filter(|f| f.name == "mp4").count(), 1);
		assert!(by("hls").mux && by("hls").demux);
		assert_eq!(by("matroska").description, "Matroska");
		// `D   matroska,webm` is one demuxer with two names
		assert!(by("webm").demux && !by("webm").mux);
		assert!(by("mov").demux && by("m4a").demux);
		assert_eq!(by("mj2").description, "QuickTime / MOV");
		assert!(formats.iter().all(|f| f.name != "lavfi"));
	}

	#[test]
	fn filters_keep_timeline_support() {
		let filters = parse_filters(FILTERS);
		let by = |name: &str| filters.iter().find(|f| f.name == name).unwrap();
		assert!(by("aap").timeline);
		assert!(!by("scale").timeline);
		assert_eq!(by("scale").description.split(' ').next(), Some("Scale"));
		assert!(by("cropdetect").timeline);
		assert_eq!(filters.len(), 12);
	}

	#[test]
	fn filters_ignore_the_legend() {
		let filters = parse_filters(FILTERS);
		assert!(filters.iter().all(|f| f.name != "=" && f.name != "|"));
	}

	#[test]
	fn hwaccels_are_one_per_line() {
		assert_eq!(
			parse_hwaccels(HWACCELS),
			["cuda", "vaapi", "qsv", "drm", "opencl", "vulkan", "amf"]
		);
		assert_eq!(parse_hwaccels("Hardware acceleration methods:\n").len(), 0);
	}

	#[test]
	fn bitstream_filters_are_one_per_line() {
		let bsfs = parse_bitstream_filters(BSFS);
		assert_eq!(bsfs.len(), 10);
		assert_eq!(bsfs[0], "aac_adtstoasc");
		assert!(bsfs.iter().any(|b| b == "h264_metadata"));
		assert!(bsfs.iter().any(|b| b == "hevc_metadata"));
		assert_eq!(parse_bitstream_filters("Bitstream filters:\n").len(), 0);
	}

	#[test]
	fn unexpected_output_parses_to_nothing() {
		let junk = "ffmpeg: error while loading shared libraries\n";
		assert_eq!(parse_codecs(junk).len(), 0);
		assert_eq!(parse_formats(junk).len(), 0);
		assert_eq!(parse_filters(junk).len(), 0);
		assert_eq!(parse_hwaccels(junk).len(), 0);
		assert_eq!(parse_bitstream_filters(junk).len(), 0);
	}
}
