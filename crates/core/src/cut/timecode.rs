//! Typed times: parsing what a user types into a cut point, and writing
//! times back the canonical way.
//!
//! Accepted forms, all with optional surrounding whitespace and a leading
//! `+`:
//!
//! | Form | Examples |
//! |---|---|
//! | Seconds, with a decimal point or comma | `754`, `754.5`, `754,5`, `.5` |
//! | Minutes and seconds | `12:34`, `12:34.567` |
//! | Hours, minutes and seconds | `0:12:34`, `00:12:34.567`, `00:12:34,567` (SRT) |
//! | Frames as a fourth group, given the frame rate | `0:12:34:12`, `0:12:34;12` |
//! | Units, in any combination in that order | `1h2m3.5s`, `90s`, `2m`, `1500ms` |
//!
//! A time is a [`Duration`], so there are no negative times: `-5` is an
//! error rather than "five seconds before the end". Every digit is kept
//! (the arithmetic is on integers), and [`format`] writes milliseconds, so
//! `parse(&format(t))` is `t` rounded to the millisecond.

use std::fmt;
use std::time::Duration;

const NANOS_PER_SEC: u128 = 1_000_000_000;

/// Why a typed time was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TimecodeError {
	/// Nothing was typed.
	#[error("type a time such as 1:23.5")]
	Empty,
	/// The text is not one of the accepted forms.
	#[error("not a time: {0}")]
	Invalid(String),
	/// The time is negative.
	#[error("a time cannot be negative")]
	Negative,
	/// Seconds or minutes of 60 or more next to a larger unit.
	#[error("{0} must be below 60")]
	Over59(&'static str),
	/// A `H:MM:SS:FF` time without a known frame rate.
	#[error("frames need the file's frame rate")]
	FramesNeedFps,
	/// The frame number is not below the frame rate.
	#[error("frame {frame} is not below the frame rate of {fps}")]
	FrameOutOfRange {
		/// The frame number typed.
		frame: u64,
		/// The frame rate, rounded up.
		fps: u64,
	},
	/// The time is past the end of the file.
	#[error("{} is past the end of the file at {}", format(*time), format(*duration))]
	BeyondEnd {
		/// The time typed.
		time: Duration,
		/// The file's length.
		duration: Duration,
	},
}

/// Parses a typed time without a frame rate; see the module docs.
///
/// # Errors
/// See [`TimecodeError`]. A frames group needs [`parse_with_fps`].
pub fn parse(text: &str) -> Result<Duration, TimecodeError> {
	parse_with_fps(text, None)
}

/// Parses a typed time; `fps` lets a fourth group count frames.
///
/// # Errors
/// See [`TimecodeError`].
pub fn parse_with_fps(text: &str, fps: Option<f64>) -> Result<Duration, TimecodeError> {
	let text = text.trim();
	if text.is_empty() {
		return Err(TimecodeError::Empty);
	}
	let text = text.strip_prefix('+').unwrap_or(text);
	if let Some(rest) = text.strip_prefix('-') {
		// Only a negative *time* is refused; "-x" is just not a time.
		return match parse_with_fps(rest, fps) {
			Ok(_) => Err(TimecodeError::Negative),
			Err(_) => Err(invalid(text)),
		};
	}
	let text = text.replace(',', ".");
	if text.contains([':', ';']) {
		parse_groups(&text, fps)
	} else if text.ends_with(|c: char| c.is_ascii_alphabetic()) {
		parse_units(&text)
	} else {
		parse_decimal(&text, 1).ok_or_else(|| invalid(&text))
	}
}

/// Checks that `time` is not past `duration`.
///
/// # Errors
/// [`TimecodeError::BeyondEnd`].
pub fn within(time: Duration, duration: Duration) -> Result<Duration, TimecodeError> {
	if time > duration {
		Err(TimecodeError::BeyondEnd { time, duration })
	} else {
		Ok(time)
	}
}

/// Writes a time as `H:MM:SS.mmm`, rounded to the millisecond: `0:12:34.567`.
#[must_use]
pub fn format(time: Duration) -> String {
	let (h, m, s, ms) = split_ms(time);
	format!("{h}:{m:02}:{s:02}.{ms:03}")
}

/// Writes a time for a file name, `HH.MM.SS.mmm`: `00.12.34.567`.
#[must_use]
pub fn format_for_filename(time: Duration) -> String {
	let (h, m, s, ms) = split_ms(time);
	format!("{h:02}.{m:02}.{s:02}.{ms:03}")
}

/// Hours, minutes, seconds and milliseconds, rounded to the millisecond.
fn split_ms(time: Duration) -> (u128, u128, u128, u128) {
	let ms = (time.as_nanos() + 500_000) / 1_000_000;
	(ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// `H:MM:SS[.f]`, `MM:SS[.f]` or `H:MM:SS:FF` (also with `;` before the
/// frames).
fn parse_groups(text: &str, fps: Option<f64>) -> Result<Duration, TimecodeError> {
	let (text, frames) = match text.rsplit_once([':', ';']) {
		Some((head, frames)) if text.matches([':', ';']).count() == 3 => (head, Some(frames)),
		_ => (text, None),
	};
	if text.contains(';') {
		return Err(invalid(text));
	}
	let groups: Vec<&str> = text.split(':').collect();
	let (hours, minutes, seconds) = match groups[..] {
		[m, s] => ("0", m, s),
		[h, m, s] => (h, m, s),
		_ => return Err(invalid(text)),
	};
	let hours = parse_decimal(hours, 3600).ok_or_else(|| invalid(text))?;
	let minutes = parse_decimal(minutes, 60).ok_or_else(|| invalid(text))?;
	let seconds = parse_decimal(seconds, 1).ok_or_else(|| invalid(text))?;
	if minutes >= Duration::from_secs(3600) && (groups.len() == 3 || frames.is_some()) {
		return Err(TimecodeError::Over59("minutes"));
	}
	if seconds >= Duration::from_secs(60) {
		return Err(TimecodeError::Over59("seconds"));
	}
	let mut total = hours + minutes + seconds;
	if let Some(frames) = frames {
		if seconds.subsec_nanos() != 0 || !frames.bytes().all(|b| b.is_ascii_digit()) {
			return Err(invalid(text));
		}
		let frame: u64 = frames.parse().map_err(|_| invalid(text))?;
		let fps = fps
			.filter(|f| f.is_finite() && *f > 0.0)
			.ok_or(TimecodeError::FramesNeedFps)?;
		#[expect(
			clippy::cast_possible_truncation,
			clippy::cast_sign_loss,
			reason = "a finite positive frame rate, rounded up"
		)]
		let whole_fps = fps.ceil() as u64;
		if frame >= whole_fps {
			return Err(TimecodeError::FrameOutOfRange {
				frame,
				fps: whole_fps,
			});
		}
		#[expect(
			clippy::cast_precision_loss,
			reason = "a frame number below the frame rate"
		)]
		let offset = frame as f64 / fps;
		total += Duration::try_from_secs_f64(offset).map_err(|_| invalid(text))?;
	}
	Ok(total)
}

/// `1h2m3.5s`, `90s`, `2m`, `1500ms`: numbers each followed by a unit.
fn parse_units(text: &str) -> Result<Duration, TimecodeError> {
	let mut total = Duration::ZERO;
	let mut rest = text;
	while !rest.is_empty() {
		let number_end = rest
			.find(|c: char| !(c.is_ascii_digit() || c == '.'))
			.ok_or_else(|| invalid(text))?;
		let (number, after) = rest.split_at(number_end);
		let unit_end = after
			.find(|c: char| !c.is_ascii_alphabetic())
			.unwrap_or(after.len());
		let (unit, after) = after.split_at(unit_end);
		let value = match unit.to_ascii_lowercase().as_str() {
			"h" => parse_decimal(number, 3600),
			"m" | "min" => parse_decimal(number, 60),
			"s" => parse_decimal(number, 1),
			"ms" => parse_decimal(number, 1).map(|d| d / 1000),
			_ => None,
		}
		.ok_or_else(|| invalid(text))?;
		total += value;
		rest = after;
	}
	Ok(total)
}

/// A non-negative decimal number of `unit` seconds (`12`, `12.5`, `.5`,
/// `12.`), exactly: the fraction's digits become nanoseconds, so nothing
/// is lost to floating point. `None` when `text` is not such a number or
/// the result would not fit.
fn parse_decimal(text: &str, unit: u64) -> Option<Duration> {
	let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
	if whole.is_empty() && fraction.is_empty() {
		return None;
	}
	if !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
		return None;
	}
	let whole: u64 = if whole.is_empty() {
		0
	} else {
		whole.parse().ok()?
	};
	// Nine digits of fraction are nanoseconds; more are dropped.
	let digits: String = fraction.chars().take(9).collect();
	let nanos: u128 = if digits.is_empty() {
		0
	} else {
		digits.parse::<u128>().ok()? * 10u128.pow(u32::try_from(9 - digits.len()).ok()?)
	};
	let total_nanos =
		u128::from(whole) * u128::from(unit) * NANOS_PER_SEC + nanos * u128::from(unit);
	let secs = u64::try_from(total_nanos / NANOS_PER_SEC).ok()?;
	let sub = u32::try_from(total_nanos % NANOS_PER_SEC).ok()?;
	Some(Duration::new(secs, sub))
}

fn invalid(text: &str) -> TimecodeError {
	TimecodeError::Invalid(text.to_owned())
}

impl fmt::Display for Timecode {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&format(self.0))
	}
}

/// A time that displays as `H:MM:SS.mmm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timecode(pub Duration);

#[cfg(test)]
mod tests {
	use super::*;
	use proptest::prelude::*;

	fn ms(ms: u64) -> Duration {
		Duration::from_millis(ms)
	}

	#[test]
	fn plain_seconds() {
		assert_eq!(parse("754").unwrap(), ms(754_000));
		assert_eq!(parse("754.5").unwrap(), ms(754_500));
		assert_eq!(parse("754,5").unwrap(), ms(754_500));
		assert_eq!(parse(" .5 ").unwrap(), ms(500));
		assert_eq!(parse("5.").unwrap(), ms(5000));
		assert_eq!(parse("+12").unwrap(), ms(12_000));
		assert_eq!(parse("0").unwrap(), Duration::ZERO);
		assert_eq!(
			parse("1.123456789123").unwrap(),
			Duration::new(1, 123_456_789)
		);
	}

	#[test]
	fn colon_groups() {
		assert_eq!(parse("12:34").unwrap(), ms(754_000));
		assert_eq!(parse("12:34.567").unwrap(), ms(754_567));
		assert_eq!(parse("0:12:34.567").unwrap(), ms(754_567));
		assert_eq!(parse("00:12:34,567").unwrap(), ms(754_567));
		assert_eq!(parse("1:00:00").unwrap(), ms(3_600_000));
		assert_eq!(parse("90:00").unwrap(), ms(5_400_000));
		assert_eq!(
			parse("1:60:00").unwrap_err(),
			TimecodeError::Over59("minutes")
		);
		assert_eq!(parse("1:60").unwrap_err(), TimecodeError::Over59("seconds"));
		assert_eq!(
			parse("1:2:3:4:5").unwrap_err(),
			TimecodeError::Invalid("1:2:3:4:5".into())
		);
		assert!(matches!(parse("1:"), Err(TimecodeError::Invalid(_))));
		assert!(matches!(parse(":5"), Err(TimecodeError::Invalid(_))));
		assert!(matches!(parse("1;2;3"), Err(TimecodeError::Invalid(_))));
	}

	#[test]
	fn frames_need_a_frame_rate() {
		assert_eq!(
			parse("0:12:34:12").unwrap_err(),
			TimecodeError::FramesNeedFps
		);
		let t = parse_with_fps("0:12:34:12", Some(25.0)).unwrap();
		assert_eq!(t, ms(754_480));
		assert_eq!(parse_with_fps("0:12:34;12", Some(25.0)).unwrap(), t);
		assert_eq!(
			parse_with_fps("0:0:0:25", Some(25.0)).unwrap_err(),
			TimecodeError::FrameOutOfRange { frame: 25, fps: 25 }
		);
		// 29.97 fps allows frames 0..=29.
		assert!(parse_with_fps("0:0:0:29", Some(29.97)).is_ok());
		assert!(matches!(
			parse_with_fps("0:0:1.5:3", Some(25.0)),
			Err(TimecodeError::Invalid(_))
		));
		assert_eq!(parse_with_fps("0:0:1", Some(0.0)).unwrap(), ms(1000));
	}

	#[test]
	fn units() {
		assert_eq!(parse("1h2m3.5s").unwrap(), ms(3_723_500));
		assert_eq!(parse("90s").unwrap(), ms(90_000));
		assert_eq!(parse("2m").unwrap(), ms(120_000));
		assert_eq!(parse("1500ms").unwrap(), ms(1500));
		assert_eq!(parse("1.5H").unwrap(), ms(5_400_000));
		assert_eq!(parse("2min").unwrap(), ms(120_000));
		assert!(matches!(parse("5x"), Err(TimecodeError::Invalid(_))));
		assert!(matches!(parse("h"), Err(TimecodeError::Invalid(_))));
		assert!(matches!(parse("1h2"), Err(TimecodeError::Invalid(_))));
	}

	#[test]
	fn rejects_empty_negative_and_garbage() {
		assert_eq!(parse("").unwrap_err(), TimecodeError::Empty);
		assert_eq!(parse("   ").unwrap_err(), TimecodeError::Empty);
		assert_eq!(parse("-5").unwrap_err(), TimecodeError::Negative);
		assert_eq!(parse("-1:00").unwrap_err(), TimecodeError::Negative);
		for bad in ["abc", "1e3", "inf", "nan", "1..2", "--1", "1 2", "１２"] {
			assert!(
				matches!(parse(bad), Err(TimecodeError::Invalid(_))),
				"{bad}"
			);
		}
		assert_eq!(
			parse("-x").unwrap_err(),
			TimecodeError::Invalid("-x".into())
		);
	}

	#[test]
	fn formats_with_milliseconds() {
		assert_eq!(format(ms(754_567)), "0:12:34.567");
		assert_eq!(format(Duration::ZERO), "0:00:00.000");
		assert_eq!(format(Duration::new(3600, 999_600_000)), "1:00:01.000");
		assert_eq!(format_for_filename(ms(754_567)), "00.12.34.567");
		assert_eq!(Timecode(ms(61_001)).to_string(), "0:01:01.001");
	}

	#[test]
	fn within_checks_the_end() {
		assert_eq!(within(ms(10), ms(10)).unwrap(), ms(10));
		let err = within(ms(11), ms(10)).unwrap_err();
		assert_eq!(
			err,
			TimecodeError::BeyondEnd {
				time: ms(11),
				duration: ms(10)
			}
		);
		assert_eq!(
			err.to_string(),
			"0:00:00.011 is past the end of the file at 0:00:00.010"
		);
	}

	proptest! {
		#[test]
		fn format_then_parse_rounds_to_the_millisecond(nanos in 0u64..3_600_000_000_000_000) {
			let time = Duration::from_nanos(nanos);
			let rounded = Duration::from_millis(u64::try_from((time.as_nanos() + 500_000) / 1_000_000).unwrap());
			prop_assert_eq!(parse(&format(time)).unwrap(), rounded);
		}

		#[test]
		fn groups_are_exact(h in 0u64..1000, m in 0u64..60, s in 0u64..60, ms in 0u64..1000) {
			let expected = Duration::from_millis(((h * 60 + m) * 60 + s) * 1000 + ms);
			prop_assert_eq!(parse(&format!("{h}:{m:02}:{s:02}.{ms:03}")).unwrap(), expected);
			prop_assert_eq!(parse(&format!("{h:02}:{m}:{s}.{ms:03}")).unwrap(), expected);
			prop_assert_eq!(parse(&format!(" +{h}:{m:02}:{s:02},{ms:03} ")).unwrap(), expected);
			prop_assert_eq!(parse(&format!("{h}h{m}m{s}.{ms:03}s")).unwrap(), expected);
			prop_assert_eq!(parse(&format!("{}.{ms:03}", (h * 60 + m) * 60 + s)).unwrap(), expected);
			if h == 0 {
				prop_assert_eq!(parse(&format!("{m}:{s:02}.{ms:03}")).unwrap(), expected);
			}
			prop_assert_eq!(parse(&format!("-{h}:{m:02}:{s:02}.{ms:03}")).unwrap_err(), TimecodeError::Negative);
		}

		#[test]
		fn frames_round_trip(h in 0u64..100, m in 0u64..60, s in 0u64..60, frame in 0u64..25) {
			let text = format!("{h}:{m:02}:{s:02}:{frame:02}");
			let expected = Duration::from_millis(((h * 60 + m) * 60 + s) * 1000 + frame * 40);
			prop_assert_eq!(parse_with_fps(&text, Some(25.0)).unwrap(), expected);
			prop_assert_eq!(parse(&text).unwrap_err(), TimecodeError::FramesNeedFps);
		}

		#[test]
		fn never_panics(text in ".*") {
			let _ = parse(&text);
			let _ = parse_with_fps(&text, Some(29.97));
		}

		#[test]
		fn seconds_match_the_float(secs in 0.0f64..1e6) {
			let text = format!("{secs:.3}");
			let parsed = parse(&text).unwrap();
			prop_assert!((parsed.as_secs_f64() - secs).abs() < 0.0005 + 1e-9, "{text} -> {parsed:?}");
		}
	}
}
