//! Splitting a file into parts by duration, size or count.
//!
//! A split is a list of [`Segment`]s that touch end to end and cover the
//! whole file; they go to the cutting engine as a keyframe cut into separate
//! files. The video of a part can only start on a keyframe, so every
//! boundary is a keyframe: [`calculate`] never cuts where the engine would
//! have to move the start back and overlap the previous part.
//!
//! - **Duration:** each part is at most the given length. A boundary is the
//!   last keyframe within that length of the part's start, so parts end up
//!   a little shorter, never longer. A GOP longer than the limit cannot be
//!   honoured and gives a part longer than it ([`Warning::PartTooLong`]).
//! - **Size:** the limit is turned into a duration with the file's average
//!   bitrate (file size over duration, container overhead and all), and the
//!   split is a duration split. Bitrate varies, so the limit is reduced by
//!   [`SIZE_SAFETY_MARGIN`] first: parts are planned to reach at most 95 %
//!   of the limit, which absorbs a part that is 5 % heavier than average. A
//!   scene much heavier than the rest of the file can still exceed that;
//!   [`Part::estimated_bytes`] is an estimate, not a measurement.
//! - **Count:** the file is cut into `n` parts of equal length, each
//!   boundary moved to the nearest keyframe. Fewer parts come out when
//!   there are too few keyframes ([`Warning::FewerParts`]).
//!
//! A file without video (`keyframes` is `None`) can be cut anywhere, so the
//! boundaries are exact.

use super::SNAP_TOLERANCE;
use super::segments::{MIN_LENGTH, Segment};

/// The fraction of a size limit a part is planned to fill; see the module
/// docs.
pub const SIZE_SAFETY_MARGIN: f64 = 0.95;
/// The most parts a split may produce, to keep a typo from writing
/// thousands of files.
pub const MAX_PARTS: usize = 1000;

/// How to split.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
	/// Parts of at most this many seconds.
	Duration(f64),
	/// Parts of at most this many bytes.
	Size(u64),
	/// This many parts of equal length.
	Count(u32),
}

/// What a split needs to know about the file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Input<'a> {
	/// Length in seconds.
	pub duration: f64,
	/// Size in bytes, when known; needed for [`Mode::Size`].
	pub size_bytes: Option<u64>,
	/// Keyframe times in seconds from the start of the file, ascending, or
	/// `None` when every point is a cut point.
	pub keyframes: Option<&'a [f64]>,
}

/// One part of the split.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
	/// From 1.
	pub number: usize,
	/// The range of the part; ranges touch end to end.
	pub segment: Segment,
	/// The size the part will have, estimated from the average bitrate;
	/// zero when the file size is unknown.
	pub estimated_bytes: u64,
}

/// Something the user should know about a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
	/// The part has no keyframe within the limit, so it is longer (or
	/// bigger) than asked.
	PartTooLong {
		/// The part's number.
		number: usize,
	},
	/// The file has too few keyframes for the number of parts asked for.
	FewerParts {
		/// What was asked.
		wanted: u32,
		/// What came out.
		got: usize,
	},
}

/// The result of [`calculate`].
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
	/// The parts, in order; never empty.
	pub parts: Vec<Part>,
	/// See [`Warning`].
	pub warnings: Vec<Warning>,
}

/// Why a split cannot be calculated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
	/// The file has no usable length.
	#[error("the file has no known duration")]
	NoDuration,
	/// A limit or count of zero, or not a number, or too small to hold a
	/// frame.
	#[error("the limit must be greater than zero, and big enough to hold a frame")]
	InvalidLimit,
	/// A size split of a file whose size is unknown.
	#[error("the file size is unknown, so a size limit cannot be applied")]
	NoSize,
	/// The split would write more than [`MAX_PARTS`] files.
	#[error("that would make more than {MAX_PARTS} parts; use a larger limit")]
	TooManyParts,
}

/// Splits a file as `mode` says.
///
/// # Errors
/// See [`Error`].
#[allow(
	clippy::cast_precision_loss,
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss
)]
pub fn calculate(mode: Mode, input: &Input<'_>) -> Result<Split, Error> {
	let duration = input.duration;
	if !duration.is_finite() || duration < MIN_LENGTH {
		return Err(Error::NoDuration);
	}
	let rate = input.size_bytes.map_or(0.0, |size| size as f64 / duration);
	let mut warnings = Vec::new();
	let bounds = match mode {
		Mode::Duration(max) => {
			if !max.is_finite() || max < MIN_LENGTH {
				return Err(Error::InvalidLimit);
			}
			by_length(max, input, &mut warnings)?
		}
		Mode::Size(limit) => {
			if limit == 0 {
				return Err(Error::InvalidLimit);
			}
			if rate <= 0.0 {
				return Err(Error::NoSize);
			}
			let max = limit as f64 * SIZE_SAFETY_MARGIN / rate;
			if max < MIN_LENGTH {
				return Err(Error::InvalidLimit);
			}
			by_length(max, input, &mut warnings)?
		}
		Mode::Count(count) => {
			if count == 0 {
				return Err(Error::InvalidLimit);
			}
			if count as usize > MAX_PARTS {
				return Err(Error::TooManyParts);
			}
			let bounds = by_count(count, input);
			if bounds.len() < count as usize {
				warnings.push(Warning::FewerParts {
					wanted: count,
					got: bounds.len(),
				});
			}
			bounds
		}
	};
	let parts = bounds
		.into_iter()
		.enumerate()
		.map(|(i, segment)| Part {
			number: i + 1,
			segment,
			estimated_bytes: (segment.length() * rate).round() as u64,
		})
		.collect();
	Ok(Split { parts, warnings })
}

/// Greedy: each part reaches the last keyframe within `max` of its start.
fn by_length(
	max: f64,
	input: &Input<'_>,
	warnings: &mut Vec<Warning>,
) -> Result<Vec<Segment>, Error> {
	let duration = input.duration;
	let mut parts = Vec::new();
	let mut start = 0.0;
	while duration - start > max + SNAP_TOLERANCE {
		if parts.len() >= MAX_PARTS {
			return Err(Error::TooManyParts);
		}
		let end = match input.keyframes {
			None => start + max,
			Some(keyframes) => {
				let reach = start + max + SNAP_TOLERANCE;
				let usable = |k: &&f64| **k > start + SNAP_TOLERANCE && **k < duration - MIN_LENGTH;
				let within = keyframes
					.iter()
					.filter(usable)
					.take_while(|k| **k <= reach)
					.last();
				if let Some(k) = within {
					*k
				} else if let Some(k) = keyframes.iter().find(usable) {
					// No keyframe within the limit: take the next one.
					warnings.push(Warning::PartTooLong {
						number: parts.len() + 1,
					});
					*k
				} else {
					break;
				}
			}
		};
		parts.push(Segment::new(start, end));
		start = end;
	}
	if duration - start > max + SNAP_TOLERANCE {
		// No keyframe is left: the rest is one long part.
		warnings.push(Warning::PartTooLong {
			number: parts.len() + 1,
		});
	}
	parts.push(Segment::new(start, duration));
	Ok(parts)
}

/// `count` parts of equal length, boundaries at the nearest keyframes.
fn by_count(count: u32, input: &Input<'_>) -> Vec<Segment> {
	let duration = input.duration;
	let target = duration / f64::from(count);
	let mut parts = Vec::new();
	let mut start = 0.0;
	for i in 1..count {
		let wanted = target * f64::from(i);
		let end = match input.keyframes {
			None => wanted,
			Some(keyframes) => {
				let nearest = keyframes
					.iter()
					.filter(|k| **k > start + SNAP_TOLERANCE && **k < duration - MIN_LENGTH)
					.min_by(|a, b| (**a - wanted).abs().total_cmp(&(**b - wanted).abs()));
				match nearest {
					Some(k) => *k,
					None => continue,
				}
			}
		};
		if end - start < MIN_LENGTH || duration - end < MIN_LENGTH {
			continue;
		}
		parts.push(Segment::new(start, end));
		start = end;
	}
	parts.push(Segment::new(start, duration));
	parts
}

#[cfg(test)]
mod tests {
	use super::*;

	fn every(step: f64, duration: f64) -> Vec<f64> {
		let mut keyframes = Vec::new();
		let mut t = 0.0;
		while t < duration {
			keyframes.push(t);
			t += step;
		}
		keyframes
	}

	fn bounds(split: &Split) -> Vec<(f64, f64)> {
		split
			.parts
			.iter()
			.map(|p| (p.segment.start, p.segment.end))
			.collect()
	}

	fn assert_contiguous(split: &Split, duration: f64) {
		assert!((split.parts[0].segment.start).abs() < 1e-9);
		for pair in split.parts.windows(2) {
			assert!((pair[0].segment.end - pair[1].segment.start).abs() < 1e-9);
		}
		let last = split.parts.last().unwrap();
		assert!((last.segment.end - duration).abs() < 1e-9);
	}

	#[test]
	fn duration_snaps_back_to_keyframes_and_keeps_a_short_last_part() {
		let keyframes = every(4.0, 100.0);
		let input = Input {
			duration: 100.0,
			size_bytes: None,
			keyframes: Some(&keyframes),
		};
		let split = calculate(Mode::Duration(30.0), &input).unwrap();
		assert_eq!(
			bounds(&split),
			vec![(0.0, 28.0), (28.0, 56.0), (56.0, 84.0), (84.0, 100.0)]
		);
		assert_eq!(split.warnings, vec![]);
		assert_contiguous(&split, 100.0);
	}

	#[test]
	fn duration_without_keyframes_is_exact() {
		let input = Input {
			duration: 100.0,
			size_bytes: None,
			keyframes: None,
		};
		let split = calculate(Mode::Duration(40.0), &input).unwrap();
		assert_eq!(
			bounds(&split),
			vec![(0.0, 40.0), (40.0, 80.0), (80.0, 100.0)]
		);
	}

	#[test]
	fn an_exact_multiple_has_no_empty_last_part() {
		let input = Input {
			duration: 90.0,
			size_bytes: None,
			keyframes: None,
		};
		let split = calculate(Mode::Duration(30.0), &input).unwrap();
		assert_eq!(split.parts.len(), 3);
		assert_contiguous(&split, 90.0);
	}

	#[test]
	fn a_file_shorter_than_the_limit_is_one_part() {
		let keyframes = [0.0, 1.0];
		let input = Input {
			duration: 2.0,
			size_bytes: Some(1000),
			keyframes: Some(&keyframes),
		};
		let split = calculate(Mode::Duration(60.0), &input).unwrap();
		assert_eq!(bounds(&split), vec![(0.0, 2.0)]);
		assert_eq!(split.parts[0].estimated_bytes, 1000);
		let split = calculate(Mode::Size(1_000_000), &input).unwrap();
		assert_eq!(split.parts.len(), 1);
	}

	#[test]
	fn a_very_short_file_still_splits_by_count() {
		let input = Input {
			duration: 0.5,
			size_bytes: None,
			keyframes: None,
		};
		let split = calculate(Mode::Count(2), &input).unwrap();
		assert_eq!(bounds(&split), vec![(0.0, 0.25), (0.25, 0.5)]);
		assert_eq!(
			calculate(
				Mode::Count(3),
				&Input {
					duration: 0.0,
					..input
				}
			),
			Err(Error::NoDuration)
		);
	}

	#[test]
	fn a_long_gop_makes_a_long_part_with_a_warning() {
		let keyframes = [0.0, 50.0, 60.0];
		let input = Input {
			duration: 70.0,
			size_bytes: None,
			keyframes: Some(&keyframes),
		};
		let split = calculate(Mode::Duration(20.0), &input).unwrap();
		assert_eq!(bounds(&split), vec![(0.0, 50.0), (50.0, 70.0)]);
		assert_eq!(split.warnings, vec![Warning::PartTooLong { number: 1 }]);
	}

	#[test]
	fn no_keyframe_left_gives_one_long_rest() {
		let keyframes = [0.0];
		let input = Input {
			duration: 70.0,
			size_bytes: None,
			keyframes: Some(&keyframes),
		};
		let split = calculate(Mode::Duration(20.0), &input).unwrap();
		assert_eq!(bounds(&split), vec![(0.0, 70.0)]);
		assert_eq!(split.warnings, vec![Warning::PartTooLong { number: 1 }]);
	}

	#[test]
	fn size_parts_stay_under_the_limit_with_the_margin() {
		// 100 s at 1000 bytes/s; limit 10 000 bytes -> 9500 bytes -> 9.5 s.
		let keyframes = every(2.0, 100.0);
		let input = Input {
			duration: 100.0,
			size_bytes: Some(100_000),
			keyframes: Some(&keyframes),
		};
		let split = calculate(Mode::Size(10_000), &input).unwrap();
		assert_contiguous(&split, 100.0);
		for part in &split.parts {
			assert!(part.estimated_bytes <= 9_500, "{part:?}");
		}
		assert_eq!(split.parts.len(), 13);
		assert_eq!(split.parts[0].segment.end, 8.0);
		let total: u64 = split.parts.iter().map(|p| p.estimated_bytes).sum();
		assert!(total.abs_diff(100_000) <= split.parts.len() as u64);
	}

	#[test]
	fn size_needs_the_file_size() {
		let input = Input {
			duration: 10.0,
			size_bytes: None,
			keyframes: None,
		};
		assert_eq!(calculate(Mode::Size(100), &input), Err(Error::NoSize));
		assert_eq!(calculate(Mode::Size(0), &input), Err(Error::InvalidLimit));
	}

	#[test]
	fn count_uses_the_nearest_keyframes() {
		let keyframes = [0.0, 10.0, 31.0, 62.0, 90.0];
		let input = Input {
			duration: 100.0,
			size_bytes: None,
			keyframes: Some(&keyframes),
		};
		let split = calculate(Mode::Count(3), &input).unwrap();
		assert_eq!(
			bounds(&split),
			vec![(0.0, 31.0), (31.0, 62.0), (62.0, 100.0)]
		);
		assert_eq!(split.warnings, vec![]);
	}

	#[test]
	fn count_with_too_few_keyframes_warns() {
		let keyframes = [0.0, 40.0];
		let input = Input {
			duration: 100.0,
			size_bytes: None,
			keyframes: Some(&keyframes),
		};
		let split = calculate(Mode::Count(4), &input).unwrap();
		assert_eq!(bounds(&split), vec![(0.0, 40.0), (40.0, 100.0)]);
		assert_eq!(
			split.warnings,
			vec![Warning::FewerParts { wanted: 4, got: 2 }]
		);
	}

	#[test]
	fn count_one_is_the_whole_file() {
		let input = Input {
			duration: 5.0,
			size_bytes: None,
			keyframes: None,
		};
		let split = calculate(Mode::Count(1), &input).unwrap();
		assert_eq!(bounds(&split), vec![(0.0, 5.0)]);
	}

	#[test]
	fn limits_are_checked() {
		let input = Input {
			duration: 10.0,
			size_bytes: Some(10),
			keyframes: None,
		};
		assert_eq!(
			calculate(Mode::Duration(0.0), &input),
			Err(Error::InvalidLimit)
		);
		assert_eq!(
			calculate(Mode::Duration(f64::NAN), &input),
			Err(Error::InvalidLimit)
		);
		assert_eq!(calculate(Mode::Count(0), &input), Err(Error::InvalidLimit));
		assert_eq!(
			calculate(Mode::Count(5000), &input),
			Err(Error::TooManyParts)
		);
		assert_eq!(
			calculate(Mode::Duration(0.002), &input),
			Err(Error::TooManyParts)
		);
	}
}
