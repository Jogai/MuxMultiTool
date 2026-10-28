//! Waveform peaks: the ffmpeg command that measures them, the parser for
//! what it reports, and the downsampling to a requested resolution.
//!
//! The analysis keeps one [`Peak`] per *bin* of a fixed length (see
//! [`TimelineConfig::peaks_per_second`]): the lowest and highest sample
//! value over all channels in that span, in -1..1. That is the base
//! resolution, measured once per stream and cached. A waveform of any
//! window at any pixel width is then a [`downsample`] of the base peaks,
//! which never touches ffmpeg again.
//!
//! ffmpeg measures the bins itself: `asetnsamples` cuts the audio into
//! bin-sized frames, `astats` computes the min and max level of each, and
//! `ametadata` prints them to stderr, where the runner's log channel picks
//! them up line by line. Nothing is written to disk, and the first bins
//! arrive while the file is still being read.
//!
//! [`TimelineConfig::peaks_per_second`]: super::TimelineConfig::peaks_per_second

use std::ops::Range;
use std::path::Path;

use crate::ffmpeg::FfmpegCommand;

/// The lowest and highest sample value in a span, in -1..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
	/// The lowest sample.
	pub min: f32,
	/// The highest sample.
	pub max: f32,
}

impl Peak {
	/// A span without a signal, which is also what a span without data shows.
	pub const SILENCE: Self = Self { min: 0.0, max: 0.0 };

	/// The envelope of both peaks.
	#[must_use]
	pub fn merge(self, other: Self) -> Self {
		Self {
			min: self.min.min(other.min),
			max: self.max.max(other.max),
		}
	}
}

/// The ffmpeg command that measures the peaks of audio stream
/// `stream_index` of `source` in bins of `samples_per_bin` samples at
/// `sample_rate` Hz. See [`PeakParser`] for what it prints.
#[must_use]
pub fn command(
	source: &Path,
	stream_index: u32,
	sample_rate: u32,
	samples_per_bin: u32,
) -> FfmpegCommand {
	FfmpegCommand::new()
		.option("-v", "repeat+info")
		.input(source)
		.option("-map", format!("0:{stream_index}"))
		.option(
			"-af",
			format!(
				"aformat=sample_fmts=fltp:sample_rates={sample_rate},asetnsamples=n={samples_per_bin},\
				 astats=metadata=1:reset=1:measure_perchannel=none:measure_overall=Min_level+Max_level,\
				 ametadata=mode=print"
			),
		)
		.option("-f", "null")
		.output("-")
}

/// Turns the stderr lines of [`command`] into peaks.
///
/// Each bin is reported as three lines (with a `[Parsed_ametadata_N @ …]`
/// prefix, which is ignored):
///
/// ```text
/// frame:12   pts:11520   pts_time:0.24
/// lavfi.astats.Overall.Min_level=-0.126848
/// lavfi.astats.Overall.Max_level=0.128298
/// ```
///
/// A peak is complete when both levels of a frame were seen. Anything else
/// in the log is ignored.
#[derive(Debug, Default)]
pub struct PeakParser {
	min: Option<f32>,
	max: Option<f32>,
}

impl PeakParser {
	/// A parser with no partial bin.
	#[must_use]
	pub fn new() -> Self {
		Self::default()
	}

	/// Feeds one line; returns the peak it completed, if any.
	pub fn push_line(&mut self, line: &str) -> Option<Peak> {
		let line = super::strip_filter_prefix(line);
		if line.starts_with("frame:") {
			self.min = None;
			self.max = None;
			return None;
		}
		let (key, value) = line.split_once('=')?;
		let slot = match key.rsplit_once('.').map_or(key, |(_, name)| name) {
			"Min_level" => &mut self.min,
			"Max_level" => &mut self.max,
			_ => return None,
		};
		*slot = Some(level(value));
		match (self.min, self.max) {
			(Some(min), Some(max)) => {
				self.min = None;
				self.max = None;
				Some(Peak {
					min: min.min(max),
					max: max.max(min),
				})
			}
			_ => None,
		}
	}
}

/// A level as astats prints it, clamped to -1..1; silence for anything
/// unreadable (`nan`, `-inf`).
fn level(text: &str) -> f32 {
	text.trim()
		.parse::<f32>()
		.ok()
		.filter(|v| v.is_finite())
		.map_or(0.0, |v| v.clamp(-1.0, 1.0))
}

/// The peaks of output bins `range` of a window that starts at `from`
/// seconds with `bin_width` seconds per output bin, from base `peaks` of
/// `bin_secs` seconds each.
///
/// Output bin `i` covers `[from + i * bin_width, from + (i + 1) * bin_width)`
/// and is the envelope of every base bin that overlaps it. A window finer
/// than the base resolution repeats base bins; a span past the end of the
/// data is [`Peak::SILENCE`].
#[must_use]
pub fn downsample(
	peaks: &[Peak],
	bin_secs: f64,
	from: f64,
	bin_width: f64,
	range: Range<usize>,
) -> Vec<Peak> {
	if bin_secs <= 0.0 || bin_width <= 0.0 {
		return vec![Peak::SILENCE; range.len()];
	}
	// Tolerance for a boundary that lands on a base bin edge, so rounding
	// does not pull in the neighbour.
	let eps = 1e-9;
	range
		.map(|i| {
			#[expect(clippy::cast_precision_loss, reason = "bin indices are small")]
			let start = from + i as f64 * bin_width;
			let end = start + bin_width;
			let first = to_index((start / bin_secs + eps).floor());
			let last = to_index((end / bin_secs - eps).ceil()).max(first + 1);
			peaks
				.get(first.min(peaks.len())..last.min(peaks.len()))
				.unwrap_or_default()
				.iter()
				.copied()
				.reduce(Peak::merge)
				.unwrap_or(Peak::SILENCE)
		})
		.collect()
}

fn to_index(value: f64) -> usize {
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		clippy::cast_precision_loss,
		reason = "clamped to the usize range first"
	)]
	let n = value.max(0.0).min(usize::MAX as f64) as usize;
	n
}

#[cfg(test)]
mod tests {
	use proptest::prelude::*;

	use super::*;

	fn peak(min: f32, max: f32) -> Peak {
		Peak { min, max }
	}

	#[test]
	fn parser_reads_a_frame_and_ignores_the_rest() {
		let mut parser = PeakParser::new();
		assert_eq!(
			parser.push_line("[Parsed_ametadata_3 @ 0x55] frame:0    pts:0       pts_time:0"),
			None
		);
		assert_eq!(
			parser
				.push_line("[Parsed_ametadata_3 @ 0x55] lavfi.astats.Overall.Min_level=-0.126848"),
			None
		);
		assert_eq!(
			parser.push_line("[Parsed_ametadata_3 @ 0x55] lavfi.astats.Overall.Max_level=0.128298"),
			Some(peak(-0.126_848, 0.128_298))
		);
		// Unprefixed lines, any order, other keys and noise in between
		assert_eq!(
			parser.push_line("frame:1    pts:9600    pts_time:0.2"),
			None
		);
		assert_eq!(parser.push_line("lavfi.astats.Overall.Max_level=0.5"), None);
		assert_eq!(
			parser.push_line("lavfi.astats.Overall.Number_of_samples=960"),
			None
		);
		assert_eq!(parser.push_line("[aac @ 0x1] decoding something"), None);
		assert_eq!(
			parser.push_line("lavfi.astats.Overall.Min_level=-0.25"),
			Some(peak(-0.25, 0.5))
		);
	}

	#[test]
	fn parser_drops_a_half_frame_and_clamps_odd_values() {
		let mut parser = PeakParser::new();
		assert_eq!(
			parser.push_line("lavfi.astats.Overall.Min_level=-0.1"),
			None
		);
		// A new frame before Max_level: the half bin is dropped
		assert_eq!(parser.push_line("frame:1 pts:1 pts_time:0.02"), None);
		assert_eq!(parser.push_line("lavfi.astats.Overall.Max_level=2.5"), None);
		assert_eq!(
			parser.push_line("lavfi.astats.Overall.Min_level=-inf"),
			Some(peak(0.0, 1.0))
		);
		assert_eq!(parser.push_line("lavfi.astats.Overall.Max_level=nan"), None);
		// A max below the min (never from astats) is put in order
		assert_eq!(
			parser.push_line("lavfi.astats.Overall.Min_level=0.3"),
			Some(peak(0.0, 0.3))
		);
	}

	#[test]
	fn downsample_merges_whole_bins() {
		let base = [
			peak(-0.1, 0.1),
			peak(-0.2, 0.2),
			peak(-0.3, 0.3),
			peak(-0.4, 0.4),
			peak(-0.5, 0.5),
			peak(-0.6, 0.6),
		];
		// 6 bins of 0.5 s into 3 of 1 s
		assert_eq!(
			downsample(&base, 0.5, 0.0, 1.0, 0..3),
			[peak(-0.2, 0.2), peak(-0.4, 0.4), peak(-0.6, 0.6)]
		);
		// A sub-range of the output
		assert_eq!(downsample(&base, 0.5, 0.0, 1.0, 1..2), [peak(-0.4, 0.4)]);
		// A window that starts later and ends past the data
		assert_eq!(
			downsample(&base, 0.5, 2.0, 1.0, 0..3),
			[peak(-0.6, 0.6), Peak::SILENCE, Peak::SILENCE]
		);
	}

	#[test]
	fn downsample_finer_than_the_base_repeats_bins() {
		let base = [peak(-0.1, 0.1), peak(-0.2, 0.2)];
		assert_eq!(
			downsample(&base, 1.0, 0.0, 0.25, 0..8),
			[
				peak(-0.1, 0.1),
				peak(-0.1, 0.1),
				peak(-0.1, 0.1),
				peak(-0.1, 0.1),
				peak(-0.2, 0.2),
				peak(-0.2, 0.2),
				peak(-0.2, 0.2),
				peak(-0.2, 0.2),
			]
		);
		// A bin straddling two base bins takes both
		assert_eq!(downsample(&base, 1.0, 0.5, 1.0, 0..1), [peak(-0.2, 0.2)]);
	}

	#[test]
	fn downsample_handles_empty_and_degenerate_input() {
		assert_eq!(downsample(&[], 0.02, 0.0, 1.0, 0..2), [Peak::SILENCE; 2]);
		assert_eq!(
			downsample(&[peak(-1.0, 1.0)], 0.0, 0.0, 1.0, 0..1),
			[Peak::SILENCE]
		);
		assert_eq!(
			downsample(&[peak(-1.0, 1.0)], 0.02, 0.0, 0.0, 0..1),
			[Peak::SILENCE]
		);
		assert_eq!(downsample(&[peak(-1.0, 1.0)], 0.02, 0.0, 1.0, 3..3), []);
	}

	#[test]
	fn boundaries_on_bin_edges_do_not_pull_in_neighbours() {
		// 0.02 s bins, 1 s window bins: 1.0 / 0.02 is 50.00000000000001
		let base: Vec<Peak> = (0..150)
			.map(|i| {
				if i < 50 {
					peak(-1.0, 1.0)
				} else {
					Peak::SILENCE
				}
			})
			.collect();
		assert_eq!(
			downsample(&base, 0.02, 0.0, 1.0, 0..3),
			[peak(-1.0, 1.0), Peak::SILENCE, Peak::SILENCE]
		);
		assert_eq!(downsample(&base, 0.02, 1.0, 1.0, 0..1), [Peak::SILENCE]);
	}

	fn peaks() -> impl Strategy<Value = Vec<Peak>> {
		prop::collection::vec(
			(-1.0f32..=1.0, -1.0f32..=1.0).prop_map(|(a, b)| peak(a.min(b), a.max(b))),
			0..64,
		)
	}

	proptest! {
		#[test]
		fn downsampled_peaks_stay_inside_the_base_envelope(
			base in peaks(),
			bin_secs in 0.001f64..1.0,
			from in 0.0f64..10.0,
			width in 0.0005f64..5.0,
			count in 0usize..40,
		) {
			let out = downsample(&base, bin_secs, from, width, 0..count);
			prop_assert_eq!(out.len(), count);
			let envelope = base.iter().copied().reduce(Peak::merge).unwrap_or(Peak::SILENCE);
			for p in out {
				prop_assert!(p.min <= p.max, "{p:?}");
				prop_assert!(p.min >= envelope.min.min(0.0), "{p:?} outside {envelope:?}");
				prop_assert!(p.max <= envelope.max.max(0.0), "{p:?} outside {envelope:?}");
			}
		}

		#[test]
		fn a_window_over_the_whole_data_at_base_resolution_is_the_data(base in peaks()) {
			let out = downsample(&base, 0.5, 0.0, 0.5, 0..base.len());
			prop_assert_eq!(out, base);
		}
	}
}
