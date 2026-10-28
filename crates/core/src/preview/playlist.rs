//! The HLS playlist of a preview session.
//!
//! The playlist is fixed when the session starts, before any segment exists:
//! it lists every segment of the whole file with its start and duration, as
//! a VOD playlist, so the player knows the duration and can seek anywhere.
//! Segments are then produced on demand to match it. For a transcode the
//! segments sit on a fixed grid, because the encoder can put a keyframe
//! exactly where each segment starts. For a stream copy a segment can only
//! start at a keyframe of the source, so the boundaries are chosen from the
//! source's keyframes, each at least a segment length after the previous.

use std::fmt::Write as _;

/// One segment: `{n}.ts` covers `start..start + duration` seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
	/// Where the segment begins, in seconds from the start of the file.
	pub start: f64,
	/// How long it is, in seconds.
	pub duration: f64,
}

impl Segment {
	/// Where the segment ends.
	#[must_use]
	pub fn end(self) -> f64 {
		self.start + self.duration
	}
}

/// All segments of a preview, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Playlist {
	segments: Vec<Segment>,
}

impl Playlist {
	/// Segments of `segment_secs` each over `duration`, the last one shorter.
	/// A trailing sliver (under a quarter of a segment) is folded into the
	/// segment before it: probed durations tend to overshoot the media by a
	/// little, and a segment ffmpeg produces nothing for would fail at the
	/// end of playback. An empty playlist when the duration is not positive.
	#[must_use]
	pub fn grid(duration: f64, segment_secs: f64) -> Self {
		let mut segments = Vec::new();
		if duration > 0.0 && segment_secs > 0.0 {
			let mut start = 0.0;
			while start < duration {
				let duration = segment_secs.min(duration - start);
				segments.push(Segment { start, duration });
				start += segment_secs;
			}
			if segments.len() > 1 && segments[segments.len() - 1].duration < segment_secs / 4.0 {
				let sliver = segments.pop().map_or(0.0, |s| s.duration);
				if let Some(last) = segments.last_mut() {
					last.duration += sliver;
				}
			}
		}
		Self { segments }
	}

	/// Segment boundaries picked from `keyframes` (ascending, in seconds).
	///
	/// Each segment starts at a keyframe and lasts at least `min_secs`: the
	/// next boundary is the first keyframe that far after the previous one.
	/// A keyframe is skipped as a boundary when another keyframe follows it
	/// within `guard_secs`, because ffmpeg's seek lands on the keyframe
	/// before the requested time minus a fixed margin; see
	/// [`command`](super::command). Returns `None` when no segment fits
	/// (no keyframes, no duration, or keyframes too dense for the guard)
	/// or when a segment would be longer than `max_secs`, so the caller
	/// can transcode instead.
	#[must_use]
	pub fn from_keyframes(
		keyframes: &[f64],
		duration: f64,
		min_secs: f64,
		max_secs: f64,
		guard_secs: f64,
	) -> Option<Self> {
		let (&first, _) = keyframes.split_first()?;
		if duration <= first {
			return None;
		}
		let mut starts = vec![first];
		let mut i = 1;
		while i < keyframes.len() {
			let candidate = keyframes[i];
			// Too close to the previous boundary.
			if candidate < starts[starts.len() - 1] + min_secs {
				i += 1;
				continue;
			}
			// A keyframe within the guard after it: the next candidate instead.
			if keyframes
				.get(i + 1)
				.is_some_and(|next| next - candidate < guard_secs)
			{
				i += 1;
				continue;
			}
			starts.push(candidate);
			i += 1;
		}
		let segments: Vec<Segment> = starts
			.iter()
			.enumerate()
			.map(|(n, &start)| {
				let end = starts.get(n + 1).copied().unwrap_or(duration).min(duration);
				Segment {
					start,
					duration: end - start,
				}
			})
			.filter(|segment| segment.duration > 0.0)
			.collect();
		if segments.is_empty() || segments.iter().any(|s| s.duration > max_secs) {
			return None;
		}
		Some(Self { segments })
	}

	/// The segments in order.
	#[must_use]
	pub fn segments(&self) -> &[Segment] {
		&self.segments
	}

	/// Segment `n`, if the playlist has one.
	#[must_use]
	pub fn get(&self, n: usize) -> Option<Segment> {
		self.segments.get(n).copied()
	}

	/// Number of segments.
	#[must_use]
	pub fn len(&self) -> usize {
		self.segments.len()
	}

	/// Whether there are no segments at all.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.segments.is_empty()
	}

	/// Total duration in seconds: where the last segment ends.
	#[must_use]
	pub fn duration(&self) -> f64 {
		self.segments.last().map_or(0.0, |s| s.end())
	}

	/// The `EXT-X-TARGETDURATION`: the longest segment, rounded up.
	#[must_use]
	pub fn target_duration(&self) -> u64 {
		#[expect(
			clippy::cast_sign_loss,
			clippy::cast_possible_truncation,
			reason = "durations are small and positive"
		)]
		self.segments
			.iter()
			.map(|s| s.duration.ceil() as u64)
			.max()
			.unwrap_or(0)
	}

	/// The playlist as `index.m3u8`: a VOD playlist with every segment, named
	/// `{n}.ts` relative to the playlist. `#EXT-X-INDEPENDENT-SEGMENTS`
	/// holds because every segment starts with a keyframe.
	#[must_use]
	pub fn to_m3u8(&self) -> String {
		let mut out = String::new();
		out.push_str("#EXTM3U\n#EXT-X-VERSION:3\n");
		let _ = writeln!(out, "#EXT-X-TARGETDURATION:{}", self.target_duration());
		out.push_str(
			"#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n",
		);
		for (n, segment) in self.segments.iter().enumerate() {
			let _ = writeln!(out, "#EXTINF:{:.6},\n{n}.ts", segment.duration);
		}
		out.push_str("#EXT-X-ENDLIST\n");
		out
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn starts(playlist: &Playlist) -> Vec<f64> {
		playlist.segments().iter().map(|s| s.start).collect()
	}

	#[test]
	fn grid_covers_the_duration_with_a_short_last_segment() {
		let playlist = Playlist::grid(7.5, 2.0);
		assert_eq!(starts(&playlist), vec![0.0, 2.0, 4.0, 6.0]);
		assert_eq!(
			playlist.get(3),
			Some(Segment {
				start: 6.0,
				duration: 1.5
			})
		);
		assert_eq!(playlist.duration(), 7.5);
		assert_eq!(playlist.target_duration(), 2);
		// A sliver at the end goes into the segment before it.
		let playlist = Playlist::grid(6.3, 2.0);
		assert_eq!(starts(&playlist), vec![0.0, 2.0, 4.0]);
		assert!((playlist.duration() - 6.3).abs() < 1e-9);
		assert_eq!(playlist.target_duration(), 3);
		assert_eq!(Playlist::grid(0.3, 2.0).len(), 1);
		assert!(Playlist::grid(0.0, 2.0).is_empty());
		assert!(Playlist::grid(-1.0, 2.0).is_empty());
		assert_eq!(Playlist::grid(2.0, 2.0).len(), 1);
	}

	#[test]
	fn keyframe_boundaries_are_at_least_a_segment_apart() {
		// Keyframes every half second; segments of at least 2 s.
		let keyframes: Vec<f64> = (0..40).map(|i| f64::from(i) * 0.5).collect();
		let playlist = Playlist::from_keyframes(&keyframes, 19.7, 2.0, 30.0, 0.15).unwrap();
		assert_eq!(
			starts(&playlist),
			vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0]
		);
		let last = playlist.get(9).unwrap();
		assert_eq!(last.start, 18.0);
		assert!((last.duration - 1.7).abs() < 1e-9);
	}

	#[test]
	fn keyframe_boundaries_follow_long_gops() {
		let playlist =
			Playlist::from_keyframes(&[0.0, 3.52, 7.0, 9.0], 12.0, 2.0, 30.0, 0.15).unwrap();
		assert_eq!(starts(&playlist), vec![0.0, 3.52, 7.0, 9.0]);
		assert_eq!(
			playlist.get(3),
			Some(Segment {
				start: 9.0,
				duration: 3.0
			})
		);
		assert_eq!(playlist.target_duration(), 4);
	}

	#[test]
	fn keyframe_within_the_guard_is_not_a_boundary() {
		// 4.0 has 4.1 right after it, so 4.1 is the boundary.
		let playlist =
			Playlist::from_keyframes(&[0.0, 4.0, 4.1, 8.0], 10.0, 2.0, 30.0, 0.15).unwrap();
		assert_eq!(starts(&playlist), vec![0.0, 4.1, 8.0]);
	}

	#[test]
	fn unusable_keyframes_give_none() {
		assert!(Playlist::from_keyframes(&[], 10.0, 2.0, 30.0, 0.15).is_none());
		assert!(Playlist::from_keyframes(&[0.0], 0.0, 2.0, 30.0, 0.15).is_none());
		// One keyframe for a long file: a 40 s segment exceeds the maximum.
		assert!(Playlist::from_keyframes(&[0.0], 40.0, 2.0, 30.0, 0.15).is_none());
		// A keyframe every frame: every candidate is inside the guard.
		let dense: Vec<f64> = (0..250).map(|i| f64::from(i) * 0.04).collect();
		assert!(Playlist::from_keyframes(&dense, 10.0, 2.0, 5.0, 0.15).is_none());
	}

	#[test]
	fn m3u8_lists_every_segment() {
		let playlist =
			Playlist::from_keyframes(&[0.0, 3.52, 7.0, 9.0], 12.0, 2.0, 30.0, 0.15).unwrap();
		insta::assert_snapshot!(playlist.to_m3u8());
	}

	#[test]
	fn m3u8_of_a_grid() {
		insta::assert_snapshot!(Playlist::grid(5.0, 2.0).to_m3u8());
	}
}
