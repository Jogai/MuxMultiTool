//! Segments: ranges of the source in seconds, and what is kept of them.
//!
//! The user marks segments in any order, possibly overlapping, possibly
//! reaching past the end of the file. [`normalize`] turns them into what
//! an export can use: clamped to the file, sorted, overlapping ones merged
//! and empty ones dropped. Segments that only touch (`[1, 2]` and
//! `[2, 3]`) stay separate, because a split into parts is exactly that;
//! [`coalesce_touching`] joins them where one piece is wanted. In
//! [`SegmentMode::Remove`] the segments are what goes, and [`invert`]
//! gives the ranges that stay.

/// One range of the source: `start` to `end` in seconds from the start of
/// the file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
	/// Where the range begins.
	pub start: f64,
	/// Where the range ends; after `start`.
	pub end: f64,
}

impl Segment {
	/// A range from `start` to `end`.
	#[must_use]
	pub const fn new(start: f64, end: f64) -> Self {
		Self { start, end }
	}

	/// `end - start`.
	#[must_use]
	pub fn length(&self) -> f64 {
		self.end - self.start
	}
}

/// What the segments mean: the parts to keep, or the parts to cut away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentMode {
	/// Export the segments.
	#[default]
	Keep,
	/// Export everything but the segments.
	Remove,
}

/// The shortest range worth exporting, in seconds. Shorter ranges (and
/// shorter gaps between removed segments) are dropped: a millisecond holds
/// no frame.
pub const MIN_LENGTH: f64 = 0.001;

/// The ranges of `segments` within a file of `duration` seconds, sorted,
/// clamped, without overlaps and without ranges shorter than
/// [`MIN_LENGTH`]. Segments that are not finite numbers are ignored.
#[must_use]
pub fn normalize(segments: &[Segment], duration: f64) -> Vec<Segment> {
	let mut clamped: Vec<Segment> = segments
		.iter()
		.filter(|s| s.start.is_finite() && s.end.is_finite())
		.map(|s| Segment::new(s.start.clamp(0.0, duration), s.end.clamp(0.0, duration)))
		.filter(|s| s.length() >= MIN_LENGTH)
		.collect();
	clamped.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.end.total_cmp(&b.end)));
	let mut merged: Vec<Segment> = Vec::with_capacity(clamped.len());
	for segment in clamped {
		match merged.last_mut() {
			Some(last) if segment.start < last.end => last.end = last.end.max(segment.end),
			_ => merged.push(segment),
		}
	}
	merged
}

/// Joins the ranges of a normalized list that touch (`[1, 2]` and
/// `[2, 3]` become `[1, 3]`), within `tolerance` seconds.
#[must_use]
pub fn coalesce_touching(normalized: &[Segment], tolerance: f64) -> Vec<Segment> {
	let mut joined: Vec<Segment> = Vec::with_capacity(normalized.len());
	for segment in normalized {
		match joined.last_mut() {
			Some(last) if segment.start - last.end <= tolerance => {
				last.end = last.end.max(segment.end);
			}
			_ => joined.push(*segment),
		}
	}
	joined
}

/// The gaps between the ranges of a normalized list, within `duration`:
/// what stays when the list is cut away. Gaps shorter than [`MIN_LENGTH`]
/// are left out.
#[must_use]
pub fn invert(normalized: &[Segment], duration: f64) -> Vec<Segment> {
	let mut gaps = Vec::with_capacity(normalized.len() + 1);
	let mut cursor = 0.0;
	for segment in normalized {
		if segment.start - cursor >= MIN_LENGTH {
			gaps.push(Segment::new(cursor, segment.start));
		}
		cursor = cursor.max(segment.end);
	}
	if duration - cursor >= MIN_LENGTH {
		gaps.push(Segment::new(cursor, duration));
	}
	gaps
}

/// The ranges an export keeps: [`normalize`]d `segments` in
/// [`SegmentMode::Keep`], their [`invert`]ed gaps in
/// [`SegmentMode::Remove`].
#[must_use]
pub fn kept_segments(segments: &[Segment], mode: SegmentMode, duration: f64) -> Vec<Segment> {
	let normalized = normalize(segments, duration);
	match mode {
		SegmentMode::Keep => normalized,
		SegmentMode::Remove => invert(&normalized, duration),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use proptest::prelude::*;

	fn seg(start: f64, end: f64) -> Segment {
		Segment::new(start, end)
	}

	#[test]
	fn normalize_sorts_merges_clamps_and_drops() {
		let input = [
			seg(50.0, 70.0),
			seg(10.0, 20.0),
			seg(15.0, 25.0),
			seg(30.0, 30.0),
			seg(40.0, 35.0),
			seg(-5.0, 2.0),
			seg(90.0, 200.0),
			seg(f64::NAN, 3.0),
			seg(1.0, f64::INFINITY),
		];
		assert_eq!(
			normalize(&input, 100.0),
			[
				seg(0.0, 2.0),
				seg(10.0, 25.0),
				seg(50.0, 70.0),
				seg(90.0, 100.0)
			]
		);
		let input = [
			seg(50.0, 70.0),
			seg(10.0, 20.0),
			seg(15.0, 25.0),
			seg(90.0, 200.0),
		];
		assert_eq!(
			normalize(&input, 100.0),
			[seg(10.0, 25.0), seg(50.0, 70.0), seg(90.0, 100.0)]
		);
		// Touching ranges stay apart; overlapping ones merge.
		assert_eq!(
			normalize(&[seg(2.0, 3.0), seg(1.0, 2.0)], 10.0),
			[seg(1.0, 2.0), seg(2.0, 3.0)]
		);
		assert_eq!(
			coalesce_touching(&[seg(1.0, 2.0), seg(2.0, 3.0), seg(3.5, 4.0)], 0.0),
			[seg(1.0, 3.0), seg(3.5, 4.0)]
		);
		assert_eq!(normalize(&[], 10.0), []);
		assert_eq!(normalize(&[seg(5.0, 5.0005)], 10.0), []);
	}

	#[test]
	fn invert_gives_the_gaps() {
		let kept = [seg(10.0, 25.0), seg(50.0, 70.0)];
		assert_eq!(
			invert(&kept, 100.0),
			[seg(0.0, 10.0), seg(25.0, 50.0), seg(70.0, 100.0)]
		);
		assert_eq!(invert(&[seg(0.0, 100.0)], 100.0), []);
		assert_eq!(invert(&[], 100.0), [seg(0.0, 100.0)]);
		assert_eq!(invert(&[seg(0.0, 40.0), seg(40.0, 100.0)], 100.0), []);
		assert_eq!(
			kept_segments(
				&[seg(30.0, 20.0), seg(70.0, 50.0)],
				SegmentMode::Remove,
				100.0
			),
			[seg(0.0, 100.0)]
		);
		assert_eq!(
			kept_segments(
				&[seg(20.0, 30.0), seg(50.0, 70.0)],
				SegmentMode::Remove,
				100.0
			),
			[seg(0.0, 20.0), seg(30.0, 50.0), seg(70.0, 100.0)]
		);
		assert_eq!(
			kept_segments(
				&[seg(50.0, 70.0), seg(20.0, 30.0)],
				SegmentMode::Keep,
				100.0
			),
			[seg(20.0, 30.0), seg(50.0, 70.0)]
		);
	}

	/// Segments on a 10 ms grid, so touching and overlapping both happen
	/// and the arithmetic is exact.
	fn grid_segments() -> impl Strategy<Value = Vec<Segment>> {
		prop::collection::vec((-50i32..1100, -50i32..1100), 0..8).prop_map(|pairs| {
			pairs
				.into_iter()
				.map(|(a, b)| seg(f64::from(a) / 100.0, f64::from(b) / 100.0))
				.collect()
		})
	}

	fn covered(segments: &[Segment], point: f64) -> bool {
		segments.iter().any(|s| s.start <= point && point <= s.end)
	}

	fn strictly_covered(segments: &[Segment], point: f64) -> bool {
		segments.iter().any(|s| s.start < point && point < s.end)
	}

	/// A duration or point on the same 10 ms grid.
	fn grid_time() -> impl Strategy<Value = f64> {
		(0i32..1000).prop_map(|t| f64::from(t) / 100.0)
	}

	fn is_normalized(segments: &[Segment], duration: f64) -> bool {
		segments
			.iter()
			.all(|s| s.start >= 0.0 && s.end <= duration && s.length() >= MIN_LENGTH)
			&& segments.windows(2).all(|w| w[0].end <= w[1].start)
	}

	proptest! {
		#[test]
		fn normalized_segments_are_sorted_disjoint_and_inside(segments in grid_segments(), duration in grid_time()) {
			let normalized = normalize(&segments, duration);
			prop_assert!(is_normalized(&normalized, duration), "{normalized:?}");
			prop_assert_eq!(normalize(&normalized, duration), normalized.clone());
		}

		#[test]
		fn normalizing_keeps_the_union(segments in grid_segments(), duration in grid_time(), point in grid_time()) {
			let point = point.min(duration);
			let clamped: Vec<Segment> = segments
				.iter()
				.map(|s| seg(s.start.clamp(0.0, duration), s.end.clamp(0.0, duration)))
				.filter(|s| s.length() >= MIN_LENGTH)
				.collect();
			let normalized = normalize(&segments, duration);
			if strictly_covered(&clamped, point) {
				prop_assert!(covered(&normalized, point), "{point} lost: {clamped:?} -> {normalized:?}");
			}
			if strictly_covered(&normalized, point) {
				prop_assert!(covered(&clamped, point), "{point} invented: {clamped:?} -> {normalized:?}");
			}
		}

		#[test]
		fn inverting_gives_the_complement(segments in grid_segments(), duration in grid_time(), point in grid_time()) {
			let point = point.min(duration);
			let normalized = normalize(&segments, duration);
			let gaps = invert(&normalized, duration);
			prop_assert!(is_normalized(&gaps, duration), "{gaps:?}");
			// Every point is in the kept ranges or in the gaps, never strictly in both.
			prop_assert!(covered(&normalized, point) || covered(&gaps, point) || duration == 0.0);
			prop_assert!(!(strictly_covered(&normalized, point) && strictly_covered(&gaps, point)));
			// The gaps of the gaps are the ranges, with touching ones joined.
			prop_assert_eq!(invert(&gaps, duration), coalesce_touching(&normalized, 0.0));
		}
	}
}
