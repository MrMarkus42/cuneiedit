use std::fmt;
use std::ops::Range;

use regex::{Captures, Regex, RegexBuilder};

use crate::{Document, LineEnding};

/// Upper bound for collected matches, so pathological patterns cannot exhaust memory.
const MAX_MATCHES: usize = 1_000_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SearchQuery {
    pub pattern: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// Escapes like `\n`, `\t`, `\x41` in pattern and replacement (ignored with `regex`).
    pub extended: bool,
    /// In regex mode, `.` also matches line breaks.
    pub dot_matches_newline: bool,
}

/// A compiled [`SearchQuery`]. Ranges are char indices; empty matches are skipped.
#[derive(Clone, Debug)]
pub struct Search {
    regex: Regex,
    expand_groups: bool,
    unescape_replacement: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchError(String);

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SearchError {}

impl Search {
    /// Returns `Ok(None)` for an empty pattern.
    pub fn new(query: &SearchQuery) -> Result<Option<Self>, SearchError> {
        if query.pattern.is_empty() {
            return Ok(None);
        }
        let extended = query.extended && !query.regex;
        let mut pattern = if query.regex {
            query.pattern.clone()
        } else if extended {
            regex::escape(&unescape(&query.pattern))
        } else {
            regex::escape(&query.pattern)
        };
        if query.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(!query.match_case)
            .multi_line(true)
            .dot_matches_new_line(query.regex && query.dot_matches_newline)
            .build()
            .map_err(|err| SearchError(err.to_string()))?;
        Ok(Some(Self {
            regex,
            expand_groups: query.regex,
            unescape_replacement: extended,
        }))
    }

    pub fn find_all(&self, doc: &Document) -> Vec<Range<usize>> {
        let text = doc.text().to_string();
        let rope = doc.text();
        self.regex
            .find_iter(&text)
            .filter(|m| !m.is_empty())
            .take(MAX_MATCHES)
            .map(|m| rope.byte_to_char(m.start())..rope.byte_to_char(m.end()))
            .collect()
    }

    /// Replaces the match spanning exactly `range` and returns the char index after it.
    pub fn replace_at(
        &self,
        doc: &mut Document,
        range: Range<usize>,
        replacement: &str,
    ) -> Option<usize> {
        let text = doc.text().to_string();
        let start = doc.text().char_to_byte(range.start);
        let end = doc.text().char_to_byte(range.end);
        let captures = self.regex.captures_at(&text, start)?;
        let found = captures.get(0)?;
        if found.start() != start || found.end() != end {
            return None;
        }
        let replacement = self.expand(&captures, replacement);
        doc.seal_undo_group();
        let after = doc.replace(range, &replacement);
        doc.seal_undo_group();
        Some(after)
    }

    /// Replaces every match (inside `within`, if given) in one undo step.
    /// Returns how many were replaced.
    pub fn replace_all(
        &self,
        doc: &mut Document,
        replacement: &str,
        within: Option<Range<usize>>,
    ) -> usize {
        let text = doc.text().to_string();
        let bounds = within.map_or(0..text.len(), |range| {
            doc.text().char_to_byte(range.start)..doc.text().char_to_byte(range.end)
        });
        let mut replaced = String::new();
        let mut span: Option<Range<usize>> = None;
        let mut count = 0;
        for captures in self.regex.captures_iter(&text) {
            let found = captures.get(0).expect("group 0 always exists");
            if found.is_empty() || found.start() < bounds.start || found.end() > bounds.end {
                continue;
            }
            match &mut span {
                Some(span) => {
                    replaced.push_str(&text[span.end..found.start()]);
                    span.end = found.end();
                }
                None => span = Some(found.range()),
            }
            replaced.push_str(&self.expand(&captures, replacement));
            count += 1;
        }
        if let Some(span) = span {
            let rope = doc.text();
            let range = rope.byte_to_char(span.start)..rope.byte_to_char(span.end);
            doc.seal_undo_group();
            doc.replace(range, &replaced);
            doc.seal_undo_group();
        }
        count
    }

    fn expand(&self, captures: &Captures<'_>, replacement: &str) -> String {
        if self.expand_groups {
            let mut out = String::new();
            captures.expand(replacement, &mut out);
            out
        } else if self.unescape_replacement {
            unescape(replacement)
        } else {
            replacement.to_owned()
        }
    }
}

/// Resolves `\n \r \t \0 \\ \xHH \uHHHH \oOOO \dDDD \bBBBBBBBB`; line breaks become `\n`
/// to match the normalized buffer. Unknown escapes are kept as they are.
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(kind) = chars.next() else {
            out.push('\\');
            break;
        };
        let (radix, digits) = match kind {
            'n' => {
                out.push('\n');
                continue;
            }
            'r' => {
                out.push('\r');
                continue;
            }
            't' => {
                out.push('\t');
                continue;
            }
            '0' => {
                out.push('\0');
                continue;
            }
            '\\' => {
                out.push('\\');
                continue;
            }
            'x' => (16, 2),
            'u' => (16, 4),
            'o' => (8, 3),
            'd' => (10, 3),
            'b' => (2, 8),
            other => {
                out.push('\\');
                out.push(other);
                continue;
            }
        };
        let code: String = (0..digits)
            .map_while(|_| chars.next_if(|d| d.is_digit(radix)))
            .collect();
        match u32::from_str_radix(&code, radix)
            .ok()
            .and_then(char::from_u32)
        {
            Some(decoded) if code.len() == digits => out.push(decoded),
            _ => {
                out.push('\\');
                out.push(kind);
                out.push_str(&code);
            }
        }
    }
    LineEnding::normalize(&out).into_owned()
}

/// Index of the first match starting at or after `pos`; with `wrap`, the first match otherwise.
pub fn next_match(matches: &[Range<usize>], pos: usize, wrap: bool) -> Option<usize> {
    let index = matches.partition_point(|m| m.start < pos);
    if index < matches.len() {
        Some(index)
    } else {
        (wrap && !matches.is_empty()).then_some(0)
    }
}

/// Index of the last match starting before `pos`; with `wrap`, the last match otherwise.
pub fn prev_match(matches: &[Range<usize>], pos: usize, wrap: bool) -> Option<usize> {
    let index = matches.partition_point(|m| m.start < pos);
    match index.checked_sub(1) {
        Some(index) => Some(index),
        None => matches.len().checked_sub(1).filter(|_| wrap),
    }
}

/// Index of the match that is exactly `range`.
pub fn match_index(matches: &[Range<usize>], range: &Range<usize>) -> Option<usize> {
    let index = matches.partition_point(|m| m.start < range.start);
    (matches.get(index) == Some(range)).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::new();
        doc.replace(0..0, text);
        doc
    }

    fn search(pattern: &str, configure: impl FnOnce(&mut SearchQuery)) -> Search {
        let mut query = SearchQuery {
            pattern: pattern.into(),
            ..SearchQuery::default()
        };
        configure(&mut query);
        Search::new(&query).unwrap().unwrap()
    }

    #[test]
    fn empty_pattern_has_no_search() {
        assert!(Search::new(&SearchQuery::default()).unwrap().is_none());
    }

    #[test]
    fn literal_search_escapes_and_ignores_case() {
        let doc = doc("Ä.b ä.b axb");
        assert_eq!(search("ä.b", |_| {}).find_all(&doc), vec![0..3, 4..7]);
        assert_eq!(
            search("ä.b", |q| q.match_case = true).find_all(&doc),
            vec![4..7]
        );
    }

    #[test]
    fn whole_word_and_regex() {
        let doc = doc("cat concat cat\ncats");
        assert_eq!(
            search("cat", |q| q.whole_word = true).find_all(&doc),
            vec![0..3, 11..14]
        );
        assert_eq!(
            search("^cat", |q| q.regex = true).find_all(&doc),
            vec![0..3, 15..18]
        );
    }

    #[test]
    fn reports_invalid_regex() {
        let query = SearchQuery {
            pattern: "(".into(),
            regex: true,
            ..SearchQuery::default()
        };
        assert!(Search::new(&query).is_err());
    }

    #[test]
    fn skips_empty_matches() {
        assert_eq!(
            search("x*", |q| q.regex = true).find_all(&doc("axxb")),
            vec![1..3]
        );
    }

    #[test]
    fn replaces_single_match_with_groups() {
        let mut doc = doc("key=value");
        let search = search(r"(\w+)=(\w+)", |q| q.regex = true);
        assert_eq!(search.replace_at(&mut doc, 0..9, "$2=$1"), Some(9));
        assert_eq!(doc.text().to_string(), "value=key");
        assert_eq!(search.replace_at(&mut doc, 0..5, "x"), None);
    }

    #[test]
    fn literal_replacement_keeps_dollar() {
        let mut doc = doc("a b a");
        assert_eq!(search("a", |_| {}).replace_all(&mut doc, "$1", None), 2);
        assert_eq!(doc.text().to_string(), "$1 b $1");
    }

    #[test]
    fn replace_all_is_one_undo_step() {
        let mut doc = doc("ä1 ä2 ä3");
        doc.seal_undo_group();
        assert_eq!(search("ä", |_| {}).replace_all(&mut doc, "ae", None), 3);
        assert_eq!(doc.text().to_string(), "ae1 ae2 ae3");
        doc.undo();
        assert_eq!(doc.text().to_string(), "ä1 ä2 ä3");
    }

    #[test]
    fn navigates_with_wrap_around() {
        let matches = [2..4, 6..8, 10..12];
        assert_eq!(next_match(&matches, 0, true), Some(0));
        assert_eq!(next_match(&matches, 4, true), Some(1));
        assert_eq!(next_match(&matches, 11, true), Some(0));
        assert_eq!(next_match(&matches, 11, false), None);
        assert_eq!(prev_match(&matches, 6, true), Some(0));
        assert_eq!(prev_match(&matches, 2, true), Some(2));
        assert_eq!(prev_match(&matches, 2, false), None);
        assert_eq!(match_index(&matches, &(6..8)), Some(1));
        assert_eq!(match_index(&matches, &(6..7)), None);
        assert_eq!(next_match(&[], 0, true), None);
    }

    #[test]
    fn unescapes_extended_sequences() {
        assert_eq!(
            unescape(r"a\tb\r\nc\x41\u00e4\o101\d065\b01000001\\"),
            "a\tb\ncAäAAA\\"
        );
        assert_eq!(unescape(r"\q\x4"), r"\q\x4");
    }

    #[test]
    fn extended_mode_finds_line_breaks_and_replaces_with_escapes() {
        let mut doc = doc("a;b\nc");
        let search = search(r"\n", |q| q.extended = true);
        assert_eq!(search.find_all(&doc), vec![3..4]);
        let search = Search::new(&SearchQuery {
            pattern: ";".into(),
            extended: true,
            ..SearchQuery::default()
        })
        .unwrap()
        .unwrap();
        search.replace_all(&mut doc, r"\t", None);
        assert_eq!(doc.text().to_string(), "a\tb\nc");
    }

    #[test]
    fn dot_can_match_newline_and_replace_within_range() {
        let text = doc("a\nb");
        assert!(search("a.b", |q| q.regex = true).find_all(&text).is_empty());
        let dotall = search("a.b", |q| {
            q.regex = true;
            q.dot_matches_newline = true;
        });
        assert_eq!(dotall.find_all(&text), vec![0..3]);

        let mut doc = doc("x x x");
        assert_eq!(
            search("x", |_| {}).replace_all(&mut doc, "y", Some(1..5)),
            2
        );
        assert_eq!(doc.text().to_string(), "x y y");
    }
}
