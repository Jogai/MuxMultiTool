//! A typed description of an ffmpeg command line.
//!
//! Operations build an [`FfmpegCommand`] and hand it to the runner, which adds
//! its own housekeeping flags (progress output, no stdin) and starts the
//! process. Keeping the operation's arguments as plain data means a snapshot
//! test can pin the exact command line of every operation.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::Path;

/// Arguments for one ffmpeg invocation, without the program name.
///
/// The builder keeps the ffmpeg vocabulary: option names include their dash
/// (`-c:v`), inputs are `-i`, and outputs are positional, so the result
/// reads like the command you would type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FfmpegCommand {
	args: Vec<OsString>,
}

impl FfmpegCommand {
	/// An empty command line.
	#[must_use]
	pub fn new() -> Self {
		Self::default()
	}

	/// Appends one raw argument.
	#[must_use]
	pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
		self.args.push(arg.as_ref().to_os_string());
		self
	}

	/// Appends several raw arguments.
	#[must_use]
	pub fn args<I, S>(mut self, args: I) -> Self
	where
		I: IntoIterator<Item = S>,
		S: AsRef<OsStr>,
	{
		self.args
			.extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
		self
	}

	/// Appends an option with a value, e.g. `option("-c:v", "libx264")`.
	#[must_use]
	pub fn option(self, name: &str, value: impl AsRef<OsStr>) -> Self {
		self.arg(name).arg(value)
	}

	/// Appends an option without a value, e.g. `flag("-y")`.
	#[must_use]
	pub fn flag(self, name: &str) -> Self {
		self.arg(name)
	}

	/// Appends an input file (`-i path`). Options that apply to this input go
	/// before it.
	#[must_use]
	pub fn input(self, path: impl AsRef<Path>) -> Self {
		self.option("-i", path.as_ref())
	}

	/// Appends an output file. Options that apply to this output go before it.
	#[must_use]
	pub fn output(self, path: impl AsRef<Path>) -> Self {
		self.arg(path.as_ref())
	}

	/// The arguments in order, without the program name.
	#[must_use]
	pub fn as_args(&self) -> &[OsString] {
		&self.args
	}
}

/// Renders the command as a shell-style line: `ffmpeg` followed by every
/// argument, quoting those a shell would misread. For logs and snapshots;
/// the runner never goes through a shell.
impl fmt::Display for FfmpegCommand {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("ffmpeg")?;
		for arg in &self.args {
			f.write_str(" ")?;
			write_quoted(f, arg)?;
		}
		Ok(())
	}
}

/// Renders a full argument list, program included, as a shell-style line
/// with the same quoting as [`FfmpegCommand`]'s `Display`. For logs and the
/// command preview of a job.
#[derive(Debug, Clone, Copy)]
pub struct ShellLine<'a>(pub &'a [OsString]);

impl fmt::Display for ShellLine<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		for (i, arg) in self.0.iter().enumerate() {
			if i > 0 {
				f.write_str(" ")?;
			}
			write_quoted(f, arg)?;
		}
		Ok(())
	}
}

/// Writes `arg` so a POSIX shell would read it back as one word.
pub(crate) fn write_quoted(f: &mut fmt::Formatter<'_>, arg: &OsStr) -> fmt::Result {
	let text = arg.to_string_lossy();
	let plain = |c: char| c.is_ascii_alphanumeric() || "-_./:=,+@%".contains(c);
	if !text.is_empty() && text.chars().all(plain) {
		f.write_str(&text)
	} else {
		write!(f, "'{}'", text.replace('\'', r"'\''"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn display_quotes_only_what_a_shell_needs() {
		let cmd = FfmpegCommand::new()
			.input("/in/a b.mkv")
			.option("-metadata", "title=it's")
			.flag("-y")
			.output("out.mp4");
		assert_eq!(
			cmd.to_string(),
			r"ffmpeg -i '/in/a b.mkv' -metadata 'title=it'\''s' -y out.mp4"
		);
	}

	#[test]
	fn shell_line_starts_with_the_program() {
		let argv: Vec<OsString> = ["/opt/mmt/ffmpeg", "-i", "a b.mkv", "out.mp4"]
			.iter()
			.map(Into::into)
			.collect();
		assert_eq!(
			ShellLine(&argv).to_string(),
			"/opt/mmt/ffmpeg -i 'a b.mkv' out.mp4"
		);
	}

	#[test]
	fn empty_argument_is_visible() {
		let cmd = FfmpegCommand::new().option("-metadata", "");
		assert_eq!(cmd.to_string(), "ffmpeg -metadata ''");
	}
}
