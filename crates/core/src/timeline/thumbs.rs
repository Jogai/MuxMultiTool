//! Thumbnail sprite sheets: the layout for a file and the ffmpeg command
//! that produces the sprites.
//!
//! One picture every `interval` seconds, scaled to a small tile, packed
//! row by row into sprite images of `columns × rows` tiles. The browser
//! fetches a sprite once and shows any of its tiles with a CSS background
//! offset, so a two-hour film costs a dozen image requests. The pictures
//! come from one ffmpeg process: the `fps` filter picks one frame per
//! interval, `tile` packs them, and the `image2` muxer writes each sprite
//! as a file, atomically, so a file that exists is complete.
//!
//! Long files are decoded keyframes-only, which is many times faster; a
//! tile then shows the last keyframe before its time, which for a timeline
//! strip is as good as the exact frame. Short files decode every frame.

use std::path::Path;

use crate::ffmpeg::FfmpegCommand;

use super::TimelineConfig;

/// JPEG quality of the sprites, ffmpeg's 1 (best) to 31 scale.
const JPEG_QUALITY: u32 = 5;

/// The layout of a file's thumbnails. See the module documentation for
/// how a time maps to a tile.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
	/// Seconds between two tiles.
	pub interval_secs: f64,
	/// Width of one tile in pixels.
	pub tile_width: u32,
	/// Height of one tile in pixels, from the picture's display aspect ratio.
	pub tile_height: u32,
	/// Tiles per sprite row.
	pub columns: u32,
	/// Rows per sprite.
	pub rows: u32,
	/// Tiles over all sprites. The plan until generation ends, then the real
	/// number, which is lower when the pictures end before the probed
	/// duration.
	pub tile_count: u32,
	/// Sprite images, `ceil(tile_count / (columns * rows))`.
	pub sprite_count: u32,
	/// Whether only keyframes are decoded.
	pub keyframes_only: bool,
}

impl Sheet {
	/// Plans the sheet of a `duration` seconds long picture of `width` ×
	/// `height` pixels with display aspect ratio `dar` (when the container
	/// says), shown at `fps` frames per second (when known).
	#[must_use]
	pub fn plan(
		duration: f64,
		width: u32,
		height: u32,
		dar: Option<f64>,
		fps: Option<f64>,
		config: &TimelineConfig,
	) -> Self {
		let interval_secs = (duration / f64::from(config.max_tiles.max(1)))
			.max(config.min_interval_secs)
			.max(0.01);
		let tile_count = ceil_to_u32(duration / interval_secs).max(1);
		let aspect = dar
			.filter(|dar| dar.is_finite() && *dar > 0.0)
			.unwrap_or_else(|| f64::from(width.max(1)) / f64::from(height.max(1)));
		let tile_width = (config.tile_width.max(16)) & !1;
		let tile_height = (round_to_u32(f64::from(tile_width) / aspect).max(2) + 1) & !1;
		let columns = config.columns.max(1);
		let rows = config.rows.max(1);
		let keyframes_only = duration * fps.unwrap_or(25.0) > config.full_decode_max_frames;
		let mut sheet = Self {
			interval_secs,
			tile_width,
			tile_height,
			columns,
			rows,
			tile_count,
			sprite_count: 0,
			keyframes_only,
		};
		sheet.sprite_count = sheet.sprites_for(tile_count);
		sheet
	}

	/// Tiles per sprite.
	#[must_use]
	pub fn per_sprite(&self) -> u32 {
		self.columns * self.rows
	}

	/// Sprites needed for `tiles` tiles.
	#[must_use]
	pub fn sprites_for(&self, tiles: u32) -> u32 {
		tiles.div_ceil(self.per_sprite())
	}

	/// The sheet with the real number of tiles, once generation has ended.
	#[must_use]
	pub fn with_tiles(mut self, tile_count: u32) -> Self {
		self.tile_count = tile_count;
		self.sprite_count = self.sprites_for(tile_count);
		self
	}
}

/// The ffmpeg command that writes the sprites of `sheet` for video stream
/// `video_index` of `source` into `dir` as `{n}.jpg`, counting from 0.
///
/// The `metadata` filters after `fps` tag every tile frame and print one
/// `frame:` line per tile to stderr, which is how the caller counts the
/// tiles. (The print filter skips frames without metadata, hence the tag.)
#[must_use]
pub fn command(source: &Path, video_index: u32, sheet: &Sheet, dir: &Path) -> FfmpegCommand {
	let mut cmd = FfmpegCommand::new().option("-v", "repeat+info");
	if sheet.keyframes_only {
		cmd = cmd.option("-skip_frame", "nokey");
	}
	cmd.input(source)
		.option("-map", format!("0:{video_index}"))
		.option(
			"-vf",
			format!(
				"fps=1/{}:start_time=0,metadata=mode=add:key=mmt.tile:value=1,\
				 metadata=mode=print:key=mmt.tile,scale={}:{},tile={}x{}",
				sheet.interval_secs, sheet.tile_width, sheet.tile_height, sheet.columns, sheet.rows
			),
		)
		.option("-q:v", JPEG_QUALITY.to_string())
		.option("-start_number", "0")
		.option("-f", "image2")
		.option("-atomic_writing", "1")
		.output(dir.join("%d.jpg"))
}

/// Whether a stderr line of the command is the `metadata` filter reporting
/// a tile.
#[must_use]
pub fn is_tile_line(line: &str) -> bool {
	super::strip_filter_prefix(line).starts_with("frame:")
}

fn ceil_to_u32(value: f64) -> u32 {
	to_u32(value.ceil())
}

fn round_to_u32(value: f64) -> u32 {
	to_u32(value.round())
}

fn to_u32(value: f64) -> u32 {
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "clamped to the u32 range first"
	)]
	let n = value.max(0.0).min(f64::from(u32::MAX)) as u32;
	n
}

#[cfg(test)]
mod tests {
	use super::*;

	fn config() -> TimelineConfig {
		TimelineConfig {
			tile_width: 160,
			columns: 10,
			rows: 10,
			max_tiles: 1200,
			min_interval_secs: 0.5,
			full_decode_max_frames: 36_000.0,
			..TimelineConfig::default()
		}
	}

	#[test]
	fn a_film_gets_the_maximum_tiles_from_keyframes() {
		let sheet = Sheet::plan(7200.0, 1920, 1080, Some(16.0 / 9.0), Some(24.0), &config());
		assert_eq!(sheet.interval_secs, 6.0);
		assert_eq!(sheet.tile_count, 1200);
		assert_eq!(sheet.sprite_count, 12);
		assert_eq!((sheet.tile_width, sheet.tile_height), (160, 90));
		assert!(sheet.keyframes_only);
	}

	#[test]
	fn a_clip_uses_the_minimum_interval_and_decodes_every_frame() {
		let sheet = Sheet::plan(4.0, 160, 120, None, Some(25.0), &config());
		assert_eq!(sheet.interval_secs, 0.5);
		assert_eq!(sheet.tile_count, 8);
		assert_eq!(sheet.sprite_count, 1);
		assert_eq!((sheet.tile_width, sheet.tile_height), (160, 120));
		assert!(!sheet.keyframes_only);
	}

	#[test]
	fn tile_sizes_are_even_and_a_partial_interval_counts() {
		// 2.35:1 at 160 wide is 68.08 high: rounded to even
		let sheet = Sheet::plan(10.1, 1920, 816, Some(2.35), None, &config());
		assert_eq!(sheet.tile_height, 68);
		// 2.2:1 is 72.7: rounded to 73, then up to even
		let sheet = Sheet::plan(10.1, 1920, 872, Some(2.2), None, &config());
		assert_eq!(sheet.tile_height, 74);
		assert_eq!(sheet.tile_count, 21);
		// Odd configured sizes are made even, a zero aspect falls back to the pixels
		let odd = TimelineConfig {
			tile_width: 161,
			..config()
		};
		let sheet = Sheet::plan(1.0, 100, 50, Some(0.0), None, &odd);
		assert_eq!((sheet.tile_width, sheet.tile_height), (160, 80));
		assert_eq!(sheet.tile_count, 2);
	}

	#[test]
	fn the_real_tile_count_shrinks_the_sprites() {
		let sheet = Sheet::plan(7200.0, 1920, 1080, None, None, &config()).with_tiles(1001);
		assert_eq!(sheet.tile_count, 1001);
		assert_eq!(sheet.sprite_count, 11);
		assert_eq!(sheet.with_tiles(0).sprite_count, 0);
	}

	#[test]
	fn sprite_command_line() {
		let sheet = Sheet::plan(7200.0, 1920, 1080, Some(16.0 / 9.0), Some(24.0), &config());
		insta::assert_snapshot!(command(
			Path::new("/media/in/movie.mkv"),
			0,
			&sheet,
			Path::new("/tmp/mmt/thumbs")
		));
	}

	#[test]
	fn clip_command_decodes_every_frame() {
		let sheet = Sheet::plan(4.0, 160, 120, None, Some(25.0), &config());
		let cmd = command(Path::new("clip.mkv"), 2, &sheet, Path::new("out")).to_string();
		assert!(!cmd.contains("-skip_frame"), "{cmd}");
		assert!(
			cmd.contains("fps=1/0.5:start_time=0,metadata=mode=add:key=mmt.tile:value=1,metadata=mode=print:key=mmt.tile,scale=160:120,tile=10x10"),
			"{cmd}"
		);
		assert!(cmd.contains("-map 0:2"), "{cmd}");
	}

	#[test]
	fn tile_lines_come_from_the_metadata_filter() {
		assert!(is_tile_line(
			"[Parsed_metadata_1 @ 0x5581] frame:0    pts:0       pts_time:0"
		));
		assert!(is_tile_line("frame:12   pts:6000    pts_time:6"));
		assert!(!is_tile_line(
			"[Parsed_metadata_1 @ 0x5581] lavfi.fps.something=1"
		));
		assert!(!is_tile_line("frame=  100 fps=0.0"));
	}
}
