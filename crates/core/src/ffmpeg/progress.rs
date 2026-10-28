//! Parser for ffmpeg's `-progress` output.
//!
//! With `-progress pipe:1` ffmpeg writes blocks of `key=value` lines to
//! stdout, one block every `-stats_period` (0.5 s by default) and one at the
//! end. A block ends with `progress=continue` or `progress=end`:
//!
//! ```text
//! frame=120
//! fps=59.85
//! bitrate=1234.5kbits/s
//! total_size=786432
//! out_time_us=4800000
//! out_time=00:00:04.800000
//! dup_frames=0
//! drop_frames=0
//! speed=2.39x
//! progress=continue
//! ```
//!
//! Any value can be `N/A`. Keys this parser does not know (`stream_0_0_q`,
//! `out_time_ms`, ...) are ignored.

use std::time::Duration;

/// One progress report, as parsed from a block of `-progress` lines.
///
/// A field is `None` when ffmpeg did not report it or reported `N/A`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
	/// Video frames written so far.
	pub frame: Option<u64>,
	/// Encoding rate in frames per second.
	pub fps: Option<f64>,
	/// Output bitrate in kbit/s.
	pub bitrate_kbps: Option<f64>,
	/// Bytes written so far.
	pub total_size: Option<u64>,
	/// Position in the output, i.e. how much media has been produced.
	pub out_time: Option<Duration>,
	/// Frames duplicated to keep the output frame rate.
	pub dup_frames: Option<u64>,
	/// Frames dropped to keep the output frame rate.
	pub drop_frames: Option<u64>,
	/// Processing speed relative to real time (`1.0` = real time).
	pub speed: Option<f64>,
	/// `true` for the last report, when ffmpeg wrote `progress=end`.
	pub end: bool,
}

/// Accumulates `-progress` lines and yields a [`Progress`] per block.
#[derive(Debug, Default)]
pub struct ProgressParser {
	current: Progress,
}

impl ProgressParser {
	/// A parser with no partial block.
	#[must_use]
	pub fn new() -> Self {
		Self::default()
	}

	/// Feeds one line. Returns the finished report when `line` ends a block.
	pub fn push_line(&mut self, line: &str) -> Option<Progress> {
		let line = line.trim();
		let (key, value) = line.split_once('=')?;
		let (key, value) = (key.trim(), value.trim());
		if key == "progress" {
			let mut done = std::mem::take(&mut self.current);
			done.end = value == "end";
			return Some(done);
		}
		let p = &mut self.current;
		match key {
			"frame" => p.frame = parse_number(value),
			"fps" => p.fps = parse_number(value),
			"bitrate" => p.bitrate_kbps = parse_unit(value, "kbits/s"),
			"total_size" => p.total_size = parse_number(value),
			// `out_time_us` is authoritative; `out_time` is the same value as
			// a timecode and only used when the microseconds are missing.
			"out_time_us" => p.out_time = parse_number(value).map(Duration::from_micros),
			"out_time" if p.out_time.is_none() => p.out_time = parse_timecode(value),
			"dup_frames" => p.dup_frames = parse_number(value),
			"drop_frames" => p.drop_frames = parse_number(value),
			"speed" => p.speed = parse_unit(value, "x"),
			_ => {}
		}
		None
	}
}

/// Parses a plain number, treating `N/A` and garbage as absent.
fn parse_number<T: std::str::FromStr>(value: &str) -> Option<T> {
	value.parse().ok()
}

/// Parses a number followed by `unit`, e.g. `2.39x` or `1234.5kbits/s`.
fn parse_unit(value: &str, unit: &str) -> Option<f64> {
	value
		.strip_suffix(unit)
		.and_then(|n| parse_number(n.trim()))
}

/// Parses ffmpeg's `HH:MM:SS.ffffff` timecode. Hours may exceed two digits.
/// A negative timecode (ffmpeg reports one before the first frame of an
/// output with a negative start) is clamped to zero.
#[must_use]
pub fn parse_timecode(value: &str) -> Option<Duration> {
	let (negative, value) = match value.strip_prefix('-') {
		Some(rest) => (true, rest),
		None => (false, value),
	};
	let mut parts = value.splitn(3, ':');
	let hours: u64 = parts.next()?.parse().ok()?;
	let minutes: u64 = parts.next()?.parse().ok()?;
	let seconds = parts.next()?;
	let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
	let whole: u64 = whole.parse().ok()?;
	if minutes >= 60
		|| whole >= 60
		|| fraction.len() > 6
		|| !fraction.bytes().all(|b| b.is_ascii_digit())
	{
		return None;
	}
	let micros = if fraction.is_empty() {
		0
	} else {
		fraction.parse::<u64>().ok()? * 10u64.pow(6 - u32::try_from(fraction.len()).ok()?)
	};
	if negative {
		return Some(Duration::ZERO);
	}
	let total_seconds = hours.checked_mul(3600)?.checked_add(minutes * 60 + whole)?;
	Some(Duration::new(
		total_seconds,
		u32::try_from(micros * 1000).ok()?,
	))
}

#[cfg(test)]
mod tests {
	use proptest::prelude::*;

	use super::*;

	fn feed(parser: &mut ProgressParser, block: &str) -> Vec<Progress> {
		block.lines().filter_map(|l| parser.push_line(l)).collect()
	}

	#[test]
	fn parses_a_block_from_ffmpeg() {
		let block = "frame=120\nfps=59.85\nstream_0_0_q=28.0\nbitrate=1234.5kbits/s\ntotal_size=786432\n\
			out_time_us=4800000\nout_time_ms=4800000\nout_time=00:00:04.800000\ndup_frames=1\n\
			drop_frames=2\nspeed=2.39x\nprogress=continue\n";
		let reports = feed(&mut ProgressParser::new(), block);
		assert_eq!(
			reports,
			vec![Progress {
				frame: Some(120),
				fps: Some(59.85),
				bitrate_kbps: Some(1234.5),
				total_size: Some(786_432),
				out_time: Some(Duration::from_millis(4800)),
				dup_frames: Some(1),
				drop_frames: Some(2),
				speed: Some(2.39),
				end: false,
			}]
		);
	}

	#[test]
	fn not_available_values_are_none_and_end_is_flagged() {
		let block = "frame=0\nfps=0.00\nbitrate=N/A\ntotal_size=N/A\nout_time_us=N/A\nout_time=N/A\n\
			speed=N/A\nprogress=end\n";
		let reports = feed(&mut ProgressParser::new(), block);
		assert_eq!(
			reports,
			vec![Progress {
				frame: Some(0),
				fps: Some(0.0),
				end: true,
				..Progress::default()
			}]
		);
	}

	#[test]
	fn out_time_is_a_fallback_for_out_time_us() {
		let mut parser = ProgressParser::new();
		let reports = feed(&mut parser, "out_time=01:02:03.5\nprogress=continue\n");
		assert_eq!(reports[0].out_time, Some(Duration::new(3723, 500_000_000)));
	}

	#[test]
	fn each_block_starts_fresh() {
		let mut parser = ProgressParser::new();
		let reports = feed(
			&mut parser,
			"frame=5\nprogress=continue\nspeed=1x\nprogress=end\n",
		);
		assert_eq!(reports.len(), 2);
		assert_eq!(reports[1].frame, None);
		assert_eq!(reports[1].speed, Some(1.0));
		assert!(reports[1].end);
	}

	#[test]
	fn timecodes() {
		assert_eq!(parse_timecode("00:00:00.000000"), Some(Duration::ZERO));
		assert_eq!(
			parse_timecode("123:59:59.999999"),
			Some(Duration::new(446_399, 999_999_000))
		);
		assert_eq!(parse_timecode("-00:00:00.023000"), Some(Duration::ZERO));
		assert_eq!(parse_timecode("00:00:01"), Some(Duration::from_secs(1)));
		assert_eq!(parse_timecode("00:60:00.0"), None);
		assert_eq!(parse_timecode("N/A"), None);
		assert_eq!(parse_timecode("1:2"), None);
		assert_eq!(parse_timecode("00:00:00.1234567"), None);
	}

	/// A report the way ffmpeg prints one. Floats have the decimals ffmpeg
	/// uses, so the generated value and its parse agree exactly.
	fn ffmpeg_report() -> impl Strategy<Value = (Progress, String)> {
		(
			any::<Option<u64>>(),
			any::<Option<u32>>(),
			any::<Option<u32>>(),
			any::<Option<u64>>(),
			prop::option::of(0u64..=400_000_000_000_000),
			any::<Option<u64>>(),
			any::<Option<u64>>(),
			any::<Option<u32>>(),
			any::<bool>(),
		)
			.prop_map(
				|(frame, fps, bitrate, size, micros, dup, dropped, speed, end)| {
					let na = String::from("N/A");
					let num = |v: Option<u64>| v.map_or_else(|| na.clone(), |v| v.to_string());
					let fixed = |v: Option<u32>, unit: &str| {
						v.map_or_else(
							|| na.clone(),
							|v| format!("{}.{:02}{unit}", v / 100, v % 100),
						)
					};
					let text = format!(
						"frame={}\nfps={}\nstream_0_0_q=-1.0\nbitrate={}\ntotal_size={}\nout_time_us={}\n\
					 out_time_ms={}\nout_time={}\ndup_frames={}\ndrop_frames={}\nspeed={}\nprogress={}\n",
						num(frame),
						fixed(fps, ""),
						fixed(bitrate, "kbits/s"),
						num(size),
						num(micros),
						num(micros),
						micros.map_or_else(|| na.clone(), timecode),
						num(dup),
						num(dropped),
						fixed(speed, "x"),
						if end { "end" } else { "continue" },
					);
					let expected = Progress {
						frame,
						fps: fps.map(|v| f64::from(v) / 100.0),
						bitrate_kbps: bitrate.map(|v| f64::from(v) / 100.0),
						total_size: size,
						out_time: micros.map(Duration::from_micros),
						dup_frames: dup,
						drop_frames: dropped,
						speed: speed.map(|v| f64::from(v) / 100.0),
						end,
					};
					(expected, text)
				},
			)
	}

	/// Formats microseconds the way ffmpeg prints `out_time`.
	fn timecode(micros: u64) -> String {
		let secs = micros / 1_000_000;
		format!(
			"{:02}:{:02}:{:02}.{:06}",
			secs / 3600,
			secs / 60 % 60,
			secs % 60,
			micros % 1_000_000
		)
	}

	proptest! {
		#[test]
		fn report_round_trips((expected, text) in ffmpeg_report()) {
			let reports = feed(&mut ProgressParser::new(), &text);
			prop_assert_eq!(reports, vec![expected]);
		}

		#[test]
		fn timecode_round_trips(micros in 0u64..=400_000_000_000_000) {
			prop_assert_eq!(parse_timecode(&timecode(micros)), Some(Duration::from_micros(micros)));
		}

		#[test]
		fn any_line_is_harmless(line in ".{0,80}") {
			let mut parser = ProgressParser::new();
			let report = parser.push_line(&line);
			// Only a `progress=` line finishes a block.
			prop_assert_eq!(report.is_some(), line.trim().starts_with("progress="));
		}

		#[test]
		fn any_timecode_text_is_harmless(text in "[-0-9:.N/A]{0,20}") {
			let _ = parse_timecode(&text);
		}
	}
}
