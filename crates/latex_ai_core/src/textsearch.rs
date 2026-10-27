//! Find and replace in text, line by line. Pure: the caller supplies the text
//! and decides which files to look at.
//!
//! Plain queries are matched literally, optionally ignoring case; a regular
//! expression uses the `regex` crate, whose engine cannot backtrack
//! catastrophically, and is built with a size limit so a hostile pattern
//! cannot take memory.

use regex::{Regex, RegexBuilder};
use serde::Serialize;

/// Longest line text sent back for a match, in characters.
const MAX_LINE_CHARS: usize = 300;
/// A pattern's compiled size may not exceed this many bytes.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

#[derive(Debug, Clone)]
pub struct Matcher {
    re: Regex,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SearchError {
    #[error("enter something to search for")]
    Empty,
    #[error("that is not a valid pattern: {0}")]
    BadPattern(String),
}

/// One match: 1-based line and column (in characters), the line it is on
/// (shortened if long) and where the match sits in that text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub line: u32,
    pub column: u32,
    pub text: String,
    /// Character offsets of the match inside `text`.
    pub start: u32,
    pub end: u32,
}

impl Matcher {
    /// # Errors
    /// [`SearchError::Empty`] for an empty query, [`SearchError::BadPattern`]
    /// for a regular expression that does not compile or is too large.
    pub fn new(query: &str, regex: bool, case_sensitive: bool) -> Result<Self, SearchError> {
        if query.is_empty() {
            return Err(SearchError::Empty);
        }
        let pattern = if regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        let re = RegexBuilder::new(&pattern)
            .case_insensitive(!case_sensitive)
            .size_limit(REGEX_SIZE_LIMIT)
            .build()
            .map_err(|e| {
                SearchError::BadPattern(e.to_string().lines().last().unwrap_or("").to_owned())
            })?;
        Ok(Self { re })
    }

    /// Every match in `text`, in order, at most `limit`. A regular expression
    /// that matches the empty string yields nothing for those matches.
    #[must_use]
    pub fn find(&self, text: &str, limit: usize) -> Vec<Hit> {
        let mut out = Vec::new();
        for (i, line) in text.lines().enumerate() {
            for m in self.re.find_iter(line) {
                if m.is_empty() {
                    continue;
                }
                if out.len() >= limit {
                    return out;
                }
                let column = line[..m.start()].chars().count();
                let len = line[m.start()..m.end()].chars().count();
                // Keep the match visible when the line is long: window the
                // text around it instead of cutting the tail off.
                let skip = column.saturating_sub(MAX_LINE_CHARS / 3);
                let shown: String = line.chars().skip(skip).take(MAX_LINE_CHARS).collect();
                out.push(Hit {
                    line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                    column: u32::try_from(column + 1).unwrap_or(u32::MAX),
                    text: shown,
                    start: u32::try_from(column - skip).unwrap_or(0),
                    end: u32::try_from((column - skip + len).min(MAX_LINE_CHARS)).unwrap_or(0),
                });
            }
        }
        out
    }

    /// `text` with every match replaced, and how many there were. `$1`-style
    /// groups work in a regular expression; a literal query's replacement is
    /// taken literally.
    #[must_use]
    pub fn replace_all(&self, text: &str, replacement: &str, literal: bool) -> (String, usize) {
        let count = self.re.find_iter(text).filter(|m| !m.is_empty()).count();
        if count == 0 {
            return (text.to_owned(), 0);
        }
        let out = if literal {
            self.re
                .replace_all(text, regex::NoExpand(replacement))
                .into_owned()
        } else {
            self.re.replace_all(text, replacement).into_owned()
        };
        (out, count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_query_is_literal_and_case_insensitive_by_default() {
        let m = Matcher::new("a.b", false, false).unwrap();
        let hits = m.find("xA.Bx\naxb\n", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].line, hits[0].column), (1, 2));
    }

    #[test]
    fn case_sensitivity_and_regex_modes() {
        assert_eq!(
            Matcher::new("ab", false, true).unwrap().find("AB", 5),
            vec![]
        );
        let m = Matcher::new(r"\\cite\{[^}]*\}", true, true).unwrap();
        assert_eq!(m.find("see \\cite{a,b} and \\cite{c}", 5).len(), 2);
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        assert_eq!(
            Matcher::new("", false, false).unwrap_err(),
            SearchError::Empty
        );
        assert!(matches!(
            Matcher::new("(", true, false),
            Err(SearchError::BadPattern(_))
        ));
        assert!(Matcher::new("(((((((((a{1000}){1000}){1000}){1000}){1000}", true, false).is_err());
    }

    #[test]
    fn the_limit_caps_the_hits_and_empty_matches_are_ignored() {
        let m = Matcher::new("a", false, false).unwrap();
        assert_eq!(m.find("aaaa\naaaa", 3).len(), 3);
        let star = Matcher::new("x*", true, false).unwrap();
        assert_eq!(star.find("abc", 5), vec![]);
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let m = Matcher::new("b", false, false).unwrap();
        let h = &m.find("éüb", 5)[0];
        assert_eq!((h.column, h.start, h.end), (3, 2, 3));
    }

    #[test]
    fn a_long_line_is_windowed_around_the_match() {
        let line = format!("{}needle{}", "x".repeat(500), "y".repeat(500));
        let h = &Matcher::new("needle", false, false).unwrap().find(&line, 5)[0];
        assert_eq!(h.column, 501);
        assert_eq!(
            &h.text
                .chars()
                .skip(h.start as usize)
                .take(6)
                .collect::<String>(),
            "needle"
        );
        assert!(h.text.chars().count() <= 300);
    }

    #[test]
    fn replacement_counts_and_respects_literal_mode() {
        let m = Matcher::new("a$", false, false).unwrap();
        let (out, n) = m.replace_all("a$ and a$", "$1", true);
        assert_eq!((out.as_str(), n), ("$1 and $1", 2));
        let r = Matcher::new(r"(\w+)@(\w+)", true, false).unwrap();
        assert_eq!(
            r.replace_all("me@host", "$2:$1", false),
            ("host:me".to_owned(), 1)
        );
        assert_eq!(m.replace_all("none", "x", true), ("none".to_owned(), 0));
    }
}
