//! The output file name template of a batch.
//!
//! A [`Template`] is a file name with placeholders in braces, e.g.
//! `{name}.mkv` or `{name}.{lang}.{date}.mp4`. [`Template::parse`] checks
//! it once for the whole batch; [`Template::render`] fills it in for one
//! file from its [`Fields`]. The rendered name is a bare file name for the
//! output folder, and one every platform can write: the template may not
//! contain a path separator; inside a field's value a separator (a language
//! tag of `a/b`, say), one of `<>:"|?*` or a control character becomes `_`;
//! trailing dots and spaces are dropped from the name, and a name Windows
//! reserves for a device (`CON`, `NUL.txt`) gets a `_` in front. The web
//! client mirrors these rules for its preview (`features/batch/lib/template.ts`).
//!
//! | Placeholder | Value                                                      |
//! |-------------|------------------------------------------------------------|
//! | `{name}`    | The input's file name without its extension                |
//! | `{ext}`     | The input's extension, without the dot                     |
//! | `{lang}`    | The languages of the kept audio streams, e.g. `eng-fre`, `und` when none is tagged |
//! | `{date}`    | The day the batch was planned, `YYYY-MM-DD` (UTC)          |
//! | `{n}`       | The file's number in the batch, 1-based, zero-padded to the count |
//! | `{parent}`  | The name of the folder the input is in                     |
//!
//! The output's extension picks the container, as it does for a single
//! stream copy, so the template normally ends in one of the container
//! extensions; `{name}.{ext}` keeps every file's own.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// The placeholders a template may use, without braces.
pub const PLACEHOLDERS: [&str; 6] = ["name", "ext", "lang", "date", "n", "parent"];

/// What is wrong with a template.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
	/// The template is empty.
	#[error("the output name template is empty")]
	Empty,
	/// The template contains `/` or `\`: outputs go into the chosen folder.
	#[error("the output name template may not contain a path separator")]
	Separator,
	/// A `{` without its `}`.
	#[error("the output name template has an unclosed {{ at position {0}")]
	Unclosed(usize),
	/// A placeholder that is not one of [`PLACEHOLDERS`].
	#[error("unknown placeholder {{{0}}}; use {list}", list = placeholders())]
	Unknown(String),
}

/// The placeholders as a list for a message: `{name}, {ext}, ...`.
fn placeholders() -> String {
	PLACEHOLDERS
		.iter()
		.map(|p| format!("{{{p}}}"))
		.collect::<Vec<_>>()
		.join(", ")
}

/// A field a placeholder stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
	/// `{name}`.
	Name,
	/// `{ext}`.
	Ext,
	/// `{lang}`.
	Lang,
	/// `{date}`.
	Date,
	/// `{n}`.
	Number,
	/// `{parent}`.
	Parent,
}

impl Field {
	fn parse(name: &str) -> Option<Self> {
		Some(match name {
			"name" => Self::Name,
			"ext" => Self::Ext,
			"lang" => Self::Lang,
			"date" => Self::Date,
			"n" => Self::Number,
			"parent" => Self::Parent,
			_ => return None,
		})
	}
}

/// One piece of a template: literal text or a placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
	Text(String),
	Field(Field),
}

/// A parsed output name template; see the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
	source: String,
	parts: Vec<Part>,
}

/// The values of one file, in the order of the placeholders.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fields {
	/// `{name}`: the input's file name without its extension.
	pub name: String,
	/// `{ext}`: the input's extension without the dot, empty when it has
	/// none.
	pub ext: String,
	/// `{lang}`: the languages of the kept audio streams joined with `-`.
	pub lang: String,
	/// `{date}`: the batch's date as `YYYY-MM-DD`.
	pub date: String,
	/// `{n}`: the file's number in the batch, already padded.
	pub number: String,
	/// `{parent}`: the name of the input's folder.
	pub parent: String,
}

impl Template {
	/// Parses `source`.
	///
	/// # Errors
	/// See [`TemplateError`].
	pub fn parse(source: &str) -> Result<Self, TemplateError> {
		if source.is_empty() {
			return Err(TemplateError::Empty);
		}
		if source.contains(['/', '\\']) {
			return Err(TemplateError::Separator);
		}
		let mut parts = Vec::new();
		let mut text = String::new();
		let mut rest = source;
		let mut position = 0;
		while let Some(open) = rest.find('{') {
			let Some(close) = rest[open..].find('}') else {
				return Err(TemplateError::Unclosed(position + open));
			};
			let name = &rest[open + 1..open + close];
			let Some(field) = Field::parse(name) else {
				return Err(TemplateError::Unknown(name.to_owned()));
			};
			text.push_str(&rest[..open]);
			if !text.is_empty() {
				parts.push(Part::Text(std::mem::take(&mut text)));
			}
			parts.push(Part::Field(field));
			position += open + close + 1;
			rest = &rest[open + close + 1..];
		}
		text.push_str(rest);
		if !text.is_empty() {
			parts.push(Part::Text(text));
		}
		Ok(Self {
			source: source.to_owned(),
			parts,
		})
	}

	/// The template as written.
	#[must_use]
	pub fn source(&self) -> &str {
		&self.source
	}

	/// Whether the template uses `field`.
	#[must_use]
	pub fn uses(&self, field: Field) -> bool {
		self.parts.contains(&Part::Field(field))
	}

	/// Fills the template in for one file: one file name every platform
	/// can write (see the module docs).
	#[must_use]
	pub fn render(&self, fields: &Fields) -> String {
		let mut name = String::new();
		for part in &self.parts {
			match part {
				Part::Text(text) => name.push_str(text),
				Part::Field(field) => {
					let value = match field {
						Field::Name => &fields.name,
						Field::Ext => &fields.ext,
						Field::Lang => &fields.lang,
						Field::Date => &fields.date,
						Field::Number => &fields.number,
						Field::Parent => &fields.parent,
					};
					name.extend(value.chars().map(safe_char));
				}
			}
		}
		safe_name(&name)
	}
}

/// `c` as a file name may hold it: `_` for a path separator, one of `<>:"|?*`
/// (reserved on Windows) or a control character.
fn safe_char(c: char) -> char {
	if matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control() {
		'_'
	} else {
		c
	}
}

/// The rendered name as one Windows can write: without trailing dots and
/// spaces (Windows drops them silently), and with a `_` in front when the
/// part before the first dot, less trailing spaces, is a device name.
fn safe_name(name: &str) -> String {
	let trimmed = name.trim_end_matches(['.', ' ']);
	let stem = trimmed
		.split('.')
		.next()
		.unwrap_or_default()
		.trim_end_matches(' ');
	if is_reserved(stem) {
		format!("_{trimmed}")
	} else {
		trimmed.to_owned()
	}
}

/// Whether `stem` is a device name Windows reserves, in any case: `CON`,
/// `PRN`, `AUX`, `NUL`, `COM0`-`COM9` or `LPT0`-`LPT9`.
fn is_reserved(stem: &str) -> bool {
	let upper = stem.to_ascii_uppercase();
	match upper.as_bytes() {
		b"CON" | b"PRN" | b"AUX" | b"NUL" => true,
		[b'C', b'O', b'M', digit] | [b'L', b'P', b'T', digit] => digit.is_ascii_digit(),
		_ => false,
	}
}

impl fmt::Display for Template {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.source)
	}
}

/// `now` as a calendar date, `YYYY-MM-DD` in UTC, for `{date}`.
#[must_use]
pub fn utc_date(now: SystemTime) -> String {
	let days = now
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_secs() / 86_400)
		.unwrap_or_default();
	let (year, month, day) = civil_from_days(i64::try_from(days).unwrap_or(i64::MAX));
	format!("{year:04}-{month:02}-{day:02}")
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
	let z = days + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z.rem_euclid(146_097);
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
	let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
	let year = yoe + era * 400 + i64::from(month <= 2);
	(year, month, day)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	fn movie() -> Fields {
		Fields {
			name: "Movie (2024)".into(),
			ext: "mp4".into(),
			lang: "eng-fre".into(),
			date: "2026-11-04".into(),
			number: "07".into(),
			parent: "rips".into(),
		}
	}

	#[test]
	fn every_placeholder_is_filled_in() {
		let template = Template::parse("{n}-{parent}-{name}.{lang}.{date}.{ext}").unwrap();
		assert_eq!(
			template.render(&movie()),
			"07-rips-Movie (2024).eng-fre.2026-11-04.mp4"
		);
		assert!(template.uses(Field::Lang));
		assert_eq!(
			template.to_string(),
			"{n}-{parent}-{name}.{lang}.{date}.{ext}"
		);
	}

	#[test]
	fn literal_text_and_repeated_placeholders_stay() {
		let template = Template::parse("{name}.mkv").unwrap();
		assert_eq!(template.render(&movie()), "Movie (2024).mkv");
		assert!(!template.uses(Field::Lang));

		let template = Template::parse("{name}-{name}").unwrap();
		assert_eq!(template.render(&movie()), "Movie (2024)-Movie (2024)");

		let template = Template::parse("fixed.mkv").unwrap();
		assert_eq!(template.render(&movie()), "fixed.mkv");
		assert_eq!(template.source(), "fixed.mkv");

		// A stray closing brace is text.
		let template = Template::parse("a}b{name}").unwrap();
		assert_eq!(template.render(&movie()), "a}bMovie (2024)");
	}

	#[test]
	fn bad_templates_are_refused() {
		assert_eq!(Template::parse("").unwrap_err(), TemplateError::Empty);
		assert_eq!(
			Template::parse("out/{name}.mkv").unwrap_err(),
			TemplateError::Separator
		);
		assert_eq!(
			Template::parse("out\\{name}.mkv").unwrap_err(),
			TemplateError::Separator
		);
		assert_eq!(
			Template::parse("{name}.{ext").unwrap_err(),
			TemplateError::Unclosed(7)
		);
		assert_eq!(
			Template::parse("{title}.mkv").unwrap_err(),
			TemplateError::Unknown("title".into())
		);
		assert_eq!(
			TemplateError::Unknown("title".into()).to_string(),
			"unknown placeholder {title}; use {name}, {ext}, {lang}, {date}, {n}, {parent}"
		);
		assert_eq!(
			TemplateError::Unclosed(3).to_string(),
			"the output name template has an unclosed { at position 3"
		);
	}

	#[test]
	fn values_cannot_add_path_components() {
		let template = Template::parse("{name}.{lang}.mkv").unwrap();
		let fields = Fields {
			name: "../up".into(),
			lang: "a/b\\c\n".into(),
			..Fields::default()
		};
		assert_eq!(template.render(&fields), ".._up.a_b_c_.mkv");
	}

	#[test]
	fn values_cannot_hold_characters_a_file_name_may_not() {
		let template = Template::parse("{name}.mkv").unwrap();
		let fields = Fields {
			name: "a<b>c:d\"e|f?g*h\u{7f}i".into(),
			..Fields::default()
		};
		assert_eq!(template.render(&fields), "a_b_c_d_e_f_g_h_i.mkv");
	}

	#[test]
	fn the_name_is_one_windows_can_write() {
		let name = |template: &str, name: &str| {
			Template::parse(template).unwrap().render(&Fields {
				name: name.into(),
				..Fields::default()
			})
		};
		// Trailing dots and spaces are dropped (Windows drops them silently).
		assert_eq!(name("{name}", "Movie. "), "Movie");
		assert_eq!(name("{name}.mkv.", "Movie"), "Movie.mkv");
		assert_eq!(name("{name}", "..."), "");
		// A reserved device name, bare or with an extension, is made ordinary.
		assert_eq!(name("{name}", "CON"), "_CON");
		assert_eq!(name("{name}.mkv", "nul"), "_nul.mkv");
		assert_eq!(name("{name}.mkv", "Com1"), "_Com1.mkv");
		assert_eq!(name("{name}.mkv", "LPT9 "), "_LPT9 .mkv");
		assert_eq!(name("{name}.mkv", "aux.old"), "_aux.old.mkv");
		assert_eq!(name("{name}.mkv", "CON "), "_CON .mkv");
		// Not reserved: a longer name, or a device name after something else.
		assert_eq!(name("{name}.mkv", "CONSOLE"), "CONSOLE.mkv");
		assert_eq!(name("{name}.mkv", "COM10"), "COM10.mkv");
		assert_eq!(name("{name}.mkv", "my CON"), "my CON.mkv");
	}

	#[test]
	fn the_date_is_the_utc_calendar_day() {
		assert_eq!(utc_date(UNIX_EPOCH), "1970-01-01");
		// 2026-11-04T23:59:59Z
		assert_eq!(
			utc_date(UNIX_EPOCH + Duration::from_secs(1_793_836_799)),
			"2026-11-04"
		);
		assert_eq!(
			utc_date(UNIX_EPOCH + Duration::from_secs(1_793_836_800)),
			"2026-11-05"
		);
		// 2000-02-29 (a leap day in a century year divisible by 400)
		assert_eq!(
			utc_date(UNIX_EPOCH + Duration::from_secs(951_782_400)),
			"2000-02-29"
		);
		assert_eq!(civil_from_days(-1), (1969, 12, 31));
	}
}
