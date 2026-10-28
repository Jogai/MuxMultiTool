//! Operations: functions that turn a request into an [`FfmpegCommand`].
//!
//! Each operation has a snapshot test of its exact command line (see
//! `snapshots/`), so a change to the arguments shows up in the PR diff. The
//! feature tasks add their operations here, next to [`remux`].

use std::path::Path;

use super::FfmpegCommand;

/// Copies every stream of `input` into `output` without re-encoding. The
/// container format follows the output file's extension.
#[must_use]
pub fn remux(input: &Path, output: &Path) -> FfmpegCommand {
	FfmpegCommand::new()
		.input(input)
		.option("-map", "0")
		.option("-c", "copy")
		.output(output)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn remux_command_line() {
		let cmd = remux(
			Path::new("/media/in/movie.mkv"),
			Path::new("/media/out/movie.mp4"),
		);
		insta::assert_snapshot!(cmd);
	}
}
