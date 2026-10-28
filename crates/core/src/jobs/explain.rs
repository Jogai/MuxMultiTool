//! Plain-language explanations of ffmpeg failures.
//!
//! ffmpeg reports problems as log lines, often several and often with the
//! useful one a few lines before the final `Conversion failed!`. The user
//! gets a one-line explanation from the patterns below and can open the raw
//! output next to it.

use std::process::ExitStatus;

/// Substrings of ffmpeg's stderr and what they mean. Earlier rows win, so
/// the specific causes come before the generic ones they are reported with.
const PATTERNS: &[(&str, &str)] = &[
	("No space left on device", "the disk is full"),
	(
		"Permission denied",
		"permission denied: the server may not read the input or write the output",
	),
	("already exists", "the output file already exists"),
	(
		"codec not currently supported in container",
		"the output container cannot hold one of the streams; pick another container or drop the stream",
	),
	(
		"Could not find tag for codec",
		"the output container cannot hold one of the streams; pick another container or drop the stream",
	),
	(
		"muxer supports only",
		"the output container cannot hold one of the streams; pick another container or drop the stream",
	),
	(
		"Error opening output",
		"the output file could not be created; check that its folder exists",
	),
	("Unknown encoder", "this ffmpeg build lacks the encoder"),
	("Unknown decoder", "this ffmpeg build lacks the decoder"),
	("Encoder not found", "this ffmpeg build lacks the encoder"),
	("Decoder not found", "this ffmpeg build lacks the decoder"),
	(
		"Unknown input format",
		"the input container is not supported by this ffmpeg build",
	),
	(
		"Invalid data found when processing input",
		"the input is not a media file ffmpeg can read",
	),
	(
		"Output file is empty",
		"no stream was selected for the output",
	),
	(
		"does not contain any stream",
		"no stream was selected for the output",
	),
	("No such file or directory", "the input file was not found"),
	("Error opening input", "the input file could not be opened"),
	// Last: the generic line ffmpeg prints after the muxer complaints above.
	(
		"Could not write header",
		"the output container cannot hold the streams as they are; pick another container or re-encode",
	),
];

/// Turns ffmpeg's exit status and the tail of its stderr into one sentence
/// for the user.
///
/// A known message maps to its explanation; anything else quotes the last
/// line ffmpeg wrote, since that is usually the reason, together with the
/// exit status.
#[must_use]
pub fn explain(status: ExitStatus, log_tail: &str) -> String {
	let known = PATTERNS
		.iter()
		.find(|(needle, _)| log_tail.contains(needle))
		.map(|(_, explanation)| (*explanation).to_owned());
	if let Some(explanation) = known {
		return explanation;
	}
	let last_line = log_tail
		.lines()
		.map(str::trim)
		.rfind(|line| !line.is_empty() && *line != "Conversion failed!");
	match (status.code(), last_line) {
		(Some(code), Some(line)) => format!("ffmpeg exited with status {code}: {line}"),
		(Some(code), None) => format!("ffmpeg exited with status {code}"),
		(None, Some(line)) => format!("ffmpeg was killed ({status}): {line}"),
		(None, None) => format!("ffmpeg was killed ({status})"),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(unix)]
	fn status(code: i32) -> ExitStatus {
		use std::os::unix::process::ExitStatusExt;
		ExitStatus::from_raw(code << 8)
	}

	#[cfg(unix)]
	#[test]
	fn known_messages_get_an_explanation() {
		let tail = "[in#0 @ 0x1] Error opening input: No such file or directory\n\
			Error opening input file /media/in/missing.mkv.\n\
			Error opening input files: No such file or directory";
		assert_eq!(explain(status(1), tail), "the input file was not found");

		let tail = "[mp4 @ 0x2] Could not find tag for codec subrip in stream #2, codec not currently supported in container\n\
			[out#0/mp4 @ 0x3] Could not write header (incorrect codec parameters ?): Invalid argument\n\
			Conversion failed!";
		assert!(explain(status(1), tail).starts_with("the output container cannot hold"));

		let tail = "File '/media/out/movie.mkv' already exists. Exiting.";
		assert_eq!(explain(status(1), tail), "the output file already exists");

		let tail = "[gif @ 0x1] gif muxer supports only codec gif for type video\n\
			[out#0/gif @ 0x2] Could not write header (incorrect codec parameters ?): Invalid argument\n\
			Conversion failed!";
		assert!(explain(status(234), tail).starts_with("the output container cannot hold one"));
		let tail = "[out#0/mp4 @ 0x2] Could not write header (incorrect codec parameters ?): Invalid argument";
		assert!(
			explain(status(234), tail).starts_with("the output container cannot hold the streams")
		);
	}

	#[cfg(unix)]
	#[test]
	fn unknown_messages_quote_the_last_useful_line() {
		let tail = "[matroska @ 0x1] something odd happened\n\nConversion failed!\n";
		assert_eq!(
			explain(status(234), tail),
			"ffmpeg exited with status 234: [matroska @ 0x1] something odd happened"
		);
		assert_eq!(explain(status(1), ""), "ffmpeg exited with status 1");
	}

	#[cfg(unix)]
	#[test]
	fn a_signal_is_reported_as_killed() {
		use std::os::unix::process::ExitStatusExt;
		let killed = ExitStatus::from_raw(9);
		assert!(
			explain(killed, "").starts_with("ffmpeg was killed ("),
			"{}",
			explain(killed, "")
		);
	}
}
