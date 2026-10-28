//! Wildcard patterns for file names, as in `*.mp4`.
//!
//! The syntax is the shell's: `*` matches any run of characters, `?` one
//! character, `[abc]` and `[a-z]` one character of a set and `[!a-z]` one
//! outside it. Matching is case-insensitive, because `*.MP4` and `*.mp4`
//! are the same files to a user. A pattern matches a whole file name, never
//! a path: `*` happily matches a `/`, so patterns are applied per name.

use std::fmt;

/// A pattern that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
	/// The pattern is empty.
	#[error("the pattern is empty")]
	Empty,
	/// A `[` has no closing `]`. A `]` right after the `[` (or the `!`) is a
	/// literal, so `[]` is unclosed as well.
	#[error("'[' without a closing ']' at position {0}")]
	UnclosedSet(usize),
}

/// A parsed wildcard pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
	source: String,
	tokens: Vec<Token>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
	/// `*`
	Any,
	/// `?`
	One,
	/// `[...]`, as inclusive ranges; a single character is a range of one.
	Set {
		negated: bool,
		ranges: Vec<(char, char)>,
	},
	Literal(char),
}

impl Token {
	fn matches(&self, c: char) -> bool {
		match self {
			Self::Any | Self::One => true,
			Self::Set { negated, ranges } => {
				ranges.iter().any(|(low, high)| (*low..=*high).contains(&c)) != *negated
			}
			Self::Literal(expected) => *expected == c,
		}
	}
}

impl Pattern {
	/// Parses `source`.
	///
	/// # Errors
	/// When the pattern is empty or a `[` set is unclosed.
	pub fn parse(source: &str) -> Result<Self, PatternError> {
		if source.is_empty() {
			return Err(PatternError::Empty);
		}
		let chars: Vec<char> = source.to_lowercase().chars().collect();
		let mut tokens = Vec::with_capacity(chars.len());
		let mut i = 0;
		while i < chars.len() {
			let token = match chars[i] {
				'*' => {
					// `**` is the same as `*`
					if tokens.last() == Some(&Token::Any) {
						i += 1;
						continue;
					}
					Token::Any
				}
				'?' => Token::One,
				'[' => {
					let (set, end) = parse_set(&chars, i)?;
					i = end;
					set
				}
				c => Token::Literal(c),
			};
			tokens.push(token);
			i += 1;
		}
		Ok(Self {
			source: source.to_owned(),
			tokens,
		})
	}

	/// The pattern as it was given.
	#[must_use]
	pub fn as_str(&self) -> &str {
		&self.source
	}

	/// Whether `name` matches, ignoring case.
	#[must_use]
	pub fn matches(&self, name: &str) -> bool {
		let name: Vec<char> = name.to_lowercase().chars().collect();
		let tokens = &self.tokens;
		let (mut t, mut n) = (0, 0);
		// Where the last `*` was and how much it has swallowed so far, so a
		// mismatch later can give it one more character and try again.
		let mut star: Option<(usize, usize)> = None;
		while n < name.len() {
			match tokens.get(t) {
				Some(Token::Any) => {
					star = Some((t, n));
					t += 1;
				}
				Some(token) if token.matches(name[n]) => {
					t += 1;
					n += 1;
				}
				_ => match star {
					Some((star_t, star_n)) => {
						star = Some((star_t, star_n + 1));
						t = star_t + 1;
						n = star_n + 1;
					}
					None => return false,
				},
			}
		}
		while tokens.get(t) == Some(&Token::Any) {
			t += 1;
		}
		t == tokens.len()
	}

	/// Whether `name` matches any of `patterns`.
	#[must_use]
	pub fn any_matches(patterns: &[Self], name: &str) -> bool {
		patterns.iter().any(|p| p.matches(name))
	}
}

impl fmt::Display for Pattern {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.source)
	}
}

/// Parses the set that opens at `chars[open]`. Returns the token and the
/// index of the closing `]`.
fn parse_set(chars: &[char], open: usize) -> Result<(Token, usize), PatternError> {
	let mut i = open + 1;
	let negated = matches!(chars.get(i), Some('!' | '^'));
	if negated {
		i += 1;
	}
	let mut ranges = Vec::new();
	// A `]` right after the opening is a literal, as in `[]a]`
	let mut first = true;
	loop {
		let Some(&c) = chars.get(i) else {
			return Err(PatternError::UnclosedSet(open));
		};
		if c == ']' && !first {
			break;
		}
		first = false;
		// `a-z`, unless the `-` is the last character of the set
		if chars.get(i + 1) == Some(&'-') && chars.get(i + 2).is_some_and(|&end| end != ']') {
			let end = chars[i + 2];
			ranges.push(if c <= end { (c, end) } else { (end, c) });
			i += 3;
		} else {
			ranges.push((c, c));
			i += 1;
		}
	}
	Ok((Token::Set { negated, ranges }, i))
}

#[cfg(test)]
mod tests {
	use super::*;
	use proptest::prelude::*;

	fn pattern(source: &str) -> Pattern {
		Pattern::parse(source).unwrap()
	}

	#[test]
	fn star_matches_any_run() {
		let p = pattern("*.mp4");
		assert!(p.matches("movie.mp4"));
		assert!(p.matches(".mp4"));
		assert!(p.matches("a.b.mp4"));
		assert!(!p.matches("movie.mkv"));
		assert!(!p.matches("movie.mp4.bak"));
		assert!(pattern("*").matches(""));
		assert!(pattern("*").matches("anything at all"));
		assert!(pattern("a*b*c").matches("axxbyyc"));
		assert!(pattern("a*b*c").matches("abc"));
		assert!(!pattern("a*b*c").matches("acb"));
	}

	#[test]
	fn question_mark_matches_one_character() {
		let p = pattern("s01e0?.mkv");
		assert!(p.matches("S01E05.mkv"));
		assert!(!p.matches("s01e05x.mkv"));
		assert!(!p.matches("s01e0.mkv"));
	}

	#[test]
	fn sets_and_ranges() {
		assert!(pattern("[abc].mp4").matches("b.mp4"));
		assert!(!pattern("[abc].mp4").matches("d.mp4"));
		assert!(pattern("ep[0-9][0-9].mp4").matches("ep42.mp4"));
		assert!(!pattern("ep[0-9][0-9].mp4").matches("ep4x.mp4"));
		assert!(pattern("[!a-m]*").matches("zebra"));
		assert!(!pattern("[!a-m]*").matches("apple"));
		assert!(pattern("[^a]").matches("b"));
		// A `-` at the end is literal, a `]` first is literal
		assert!(pattern("[a-]").matches("-"));
		assert!(pattern("[]a]").matches("]"));
		// A reversed range still works
		assert!(pattern("[z-a]").matches("m"));
	}

	#[test]
	fn matching_ignores_case() {
		assert!(pattern("*.MP4").matches("movie.mp4"));
		assert!(pattern("*.mp4").matches("MOVIE.MP4"));
		assert!(pattern("[A-C]").matches("b"));
		assert!(pattern("ÉPISODE*").matches("épisode 1.mkv"));
	}

	#[test]
	fn literal_names_match_themselves_only() {
		assert!(pattern("movie.mp4").matches("movie.mp4"));
		assert!(!pattern("movie.mp4").matches("movie.mp4 "));
		assert!(!pattern("movie.mp4").matches("ovie.mp4"));
		// A slash is an ordinary character: patterns are applied per name
		assert!(pattern("a/b").matches("a/b"));
	}

	#[test]
	fn invalid_patterns_are_rejected() {
		assert_eq!(Pattern::parse(""), Err(PatternError::Empty));
		assert_eq!(Pattern::parse("*.[mp4"), Err(PatternError::UnclosedSet(2)));
		assert_eq!(Pattern::parse("x[]"), Err(PatternError::UnclosedSet(1)));
		assert_eq!(Pattern::parse("[!]"), Err(PatternError::UnclosedSet(0)));
		assert!(pattern("[!]]").matches("x"));
		assert!(!pattern("[!]]").matches("]"));
	}

	#[test]
	fn any_matches_takes_the_first_hit() {
		let patterns = [pattern("*.mp4"), pattern("*.mkv")];
		assert!(Pattern::any_matches(&patterns, "a.mkv"));
		assert!(!Pattern::any_matches(&patterns, "a.avi"));
		assert!(!Pattern::any_matches(&[], "a.mp4"));
	}

	#[test]
	fn display_keeps_the_source() {
		assert_eq!(pattern("*.MP4").to_string(), "*.MP4");
		assert_eq!(pattern("*.MP4").as_str(), "*.MP4");
	}

	/// File names without wildcard characters.
	fn plain_name() -> impl Strategy<Value = String> {
		"[a-zA-Z0-9 ._-]{0,12}"
	}

	proptest! {
		#[test]
		fn a_plain_name_matches_itself(name in "[a-zA-Z0-9 ._-]{1,12}") {
			prop_assert!(pattern(&name).matches(&name));
		}

		#[test]
		fn a_pattern_with_the_name_as_prefix_or_suffix(name in "[a-zA-Z0-9 ._-]{1,12}", rest in plain_name()) {
			let (prefix, suffix, infix) = (format!("{name}*"), format!("*{name}"), format!("*{name}*"));
			let (at_start, at_end, inside) = (format!("{name}{rest}"), format!("{rest}{name}"), format!("{rest}{name}{rest}"));
			prop_assert!(pattern(&prefix).matches(&at_start));
			prop_assert!(pattern(&suffix).matches(&at_end));
			prop_assert!(pattern(&infix).matches(&inside));
		}

		#[test]
		fn question_marks_match_any_name_of_that_length(name in "[a-zA-Z0-9 ._-]{1,12}") {
			let same_length = "?".repeat(name.chars().count());
			prop_assert!(pattern(&same_length).matches(&name));
			let one_longer = format!("{same_length}?");
			prop_assert!(!pattern(&one_longer).matches(&name));
		}

		#[test]
		fn star_matches_everything(name in "\\PC{0,20}") {
			prop_assert!(pattern("*").matches(&name));
			prop_assert!(pattern("**").matches(&name));
		}

		#[test]
		fn parsing_never_panics(source in "\\PC{0,20}") {
			let _ = Pattern::parse(&source);
		}
	}
}
