//! The ffmpeg command of a merge and the two files it reads besides the
//! parts, each with a snapshot test.
//!
//! The concat demuxer reads a list file (`file '…'` lines) and presents the
//! parts as one input, so `-c copy` joins them packet by packet. `-safe 0`
//! allows absolute paths in the list. A dropped stream is a negative map
//! (`-map -0:N`) after `-map 0`. Chapters come from a second input in
//! ffmpeg's metadata format with one `[CHAPTER]` per part, taken over with
//! `-map_chapters 1`; the first part's own metadata is kept, since nothing
//! maps the second input's.
//!
//! The list and the chapters are plain text that the caller writes
//! somewhere ffmpeg can read when the job runs; see [`concat_list`] and
//! [`chapters`].

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use super::Part;
use crate::ffmpeg::FfmpegCommand;

/// The concat demuxer's list of `parts`, one `file` line each. A `'` in a
/// path is written as `'\''`, as a shell would.
#[must_use]
pub fn concat_list(parts: &[Part]) -> String {
	let mut list = String::new();
	for part in parts {
		let path = part.path.to_string_lossy().replace('\'', r"'\''");
		let _ = writeln!(list, "file '{path}'");
	}
	list
}

/// Chapters in ffmpeg's metadata format: one per part, named after the
/// file without its extension, back to back from 0. `None` when a part's
/// duration is unknown, since the chapter after it would have no start.
#[must_use]
pub fn chapters(parts: &[Part]) -> Option<String> {
	let mut text = String::from(";FFMETADATA1\n");
	let mut start = Duration::ZERO;
	for part in parts {
		let end = start + part.duration?;
		let title = Path::new(&part.name)
			.file_stem()
			.map_or_else(|| part.name.clone(), |s| s.to_string_lossy().into_owned());
		let _ = write!(
			text,
			"[CHAPTER]\nTIMEBASE=1/1000\nSTART={}\nEND={}\ntitle={}\n",
			start.as_millis(),
			end.as_millis(),
			escape(&title)
		);
		start = end;
	}
	Some(text)
}

/// Escapes a metadata value: `=`, `;`, `#`, `\` and newlines are special
/// in ffmpeg's metadata format.
fn escape(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	for c in value.chars() {
		match c {
			'=' | ';' | '#' | '\\' => {
				out.push('\\');
				out.push(c);
			}
			'\n' => out.push_str("\\\n"),
			_ => out.push(c),
		}
	}
	out
}

/// The command that joins the parts in `list` into `output`, leaving out
/// the streams in `drop`, with the chapters of `chapters` when given.
#[must_use]
pub fn merge(list: &Path, chapters: Option<&Path>, drop: &[u32], output: &Path) -> FfmpegCommand {
	let mut cmd = FfmpegCommand::new()
		.option("-f", "concat")
		.option("-safe", "0")
		.input(list);
	if let Some(chapters) = chapters {
		cmd = cmd.option("-f", "ffmetadata").input(chapters);
	}
	cmd = cmd.option("-map", "0");
	for index in drop {
		cmd = cmd.option("-map", format!("-0:{index}"));
	}
	if chapters.is_some() {
		cmd = cmd.option("-map_chapters", "1");
	}
	cmd.option("-c", "copy").output(output)
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use super::*;

	fn parts() -> Vec<Part> {
		["Show S01E01.mkv", "Show S01E02.mkv", "It's S01E03.mkv"]
			.iter()
			.enumerate()
			.map(|(i, name)| Part {
				path: PathBuf::from("/media/Series/Show").join(name),
				name: (*name).to_owned(),
				duration: Some(Duration::from_millis(1000 * (i as u64 + 1) + 500)),
				streams: Vec::new(),
			})
			.collect()
	}

	#[test]
	fn the_concat_list_quotes_paths() {
		insta::assert_snapshot!(concat_list(&parts()));
	}

	#[test]
	fn chapters_follow_each_other_and_are_named_after_the_files() {
		insta::assert_snapshot!(chapters(&parts()).unwrap());
	}

	#[test]
	fn chapters_need_every_duration() {
		let mut parts = parts();
		parts[1].duration = None;
		assert_eq!(chapters(&parts), None);
	}

	#[test]
	fn metadata_values_are_escaped() {
		assert_eq!(escape("a=b;c#d\\e\nf"), "a\\=b\\;c\\#d\\\\e\\\nf");
	}

	#[test]
	fn merge_copies_every_stream() {
		let cmd = merge(
			Path::new("/tmp/mmt/merge/1a2b.txt"),
			None,
			&[],
			Path::new("/media/Series/Show/Show S01E01-E03.mkv"),
		);
		insta::assert_snapshot!(cmd);
	}

	#[test]
	fn merge_with_dropped_streams_and_chapters() {
		let cmd = merge(
			Path::new("/tmp/mmt/merge/1a2b.txt"),
			Some(Path::new("/tmp/mmt/merge/1a2b.ffmeta")),
			&[1, 3],
			Path::new("/media/Series/Show/Show S01E01-E03.mkv"),
		);
		insta::assert_snapshot!(cmd);
	}
}
