//! Line and selection operations. Each one is a single undo step and returns the new selection.

use std::collections::HashSet;
use std::ops::Range;

use crate::Document;

/// Lines touched by `selection`. A selection ending at a line start does not include that line.
pub fn selected_lines(doc: &Document, selection: &Range<usize>) -> Range<usize> {
    let rope = doc.text();
    let first = rope.char_to_line(selection.start);
    let mut last = rope.char_to_line(selection.end);
    if selection.end > selection.start && last > first && rope.line_to_char(last) == selection.end {
        last -= 1;
    }
    first..last + 1
}

/// Char range of `lines` without the final line break.
fn block(doc: &Document, lines: &Range<usize>) -> Range<usize> {
    let rope = doc.text();
    let start = rope.line_to_char(lines.start);
    let end = if lines.end < rope.len_lines() {
        rope.line_to_char(lines.end) - 1
    } else {
        rope.len_chars()
    };
    start..end
}

/// Replaces `range` as one undo step and returns the range of the new text.
fn replace(doc: &mut Document, range: Range<usize>, text: &str) -> Range<usize> {
    doc.seal_undo_group();
    let end = doc.replace(range.clone(), text);
    doc.seal_undo_group();
    range.start..end
}

fn map_lines(
    doc: &mut Document,
    selection: &Range<usize>,
    f: impl FnOnce(Vec<&str>) -> Vec<String>,
) -> Range<usize> {
    let range = block(doc, &selected_lines(doc, selection));
    let text = doc.slice(range.clone());
    let new = f(text.split('\n').collect()).join("\n");
    if new == text {
        return selection.clone();
    }
    replace(doc, range, &new)
}

/// Lines of the selection, or of the whole document if at most one line is selected.
fn lines_or_all(doc: &Document, selection: &Range<usize>) -> Range<usize> {
    if selected_lines(doc, selection).len() > 1 {
        selection.clone()
    } else {
        0..doc.text().len_chars()
    }
}

/// Keeps only the lines for which `keep(line)` is true, as one undo step.
/// Returns the number of removed lines.
pub fn retain_lines(doc: &mut Document, mut keep: impl FnMut(usize) -> bool) -> usize {
    let text = doc.text().to_string();
    let lines: Vec<&str> = text.split('\n').collect();
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|&(line, _)| keep(line))
        .map(|(_, text)| *text)
        .collect();
    let removed = lines.len() - kept.len();
    if removed > 0 {
        replace(doc, 0..doc.text().len_chars(), &kept.join("\n"));
    }
    removed
}

pub fn duplicate_lines(doc: &mut Document, selection: &Range<usize>) -> Range<usize> {
    let range = block(doc, &selected_lines(doc, selection));
    let text = doc.slice(range.clone());
    let inserted = replace(doc, range.end..range.end, &format!("\n{text}"));
    inserted.start + 1..inserted.end
}

pub fn delete_lines(doc: &mut Document, selection: &Range<usize>) -> Range<usize> {
    let lines = selected_lines(doc, selection);
    let rope = doc.text();
    let start = rope.line_to_char(lines.start);
    let range = if lines.end < rope.len_lines() {
        start..rope.line_to_char(lines.end)
    } else if lines.start > 0 {
        start - 1..rope.len_chars()
    } else {
        0..rope.len_chars()
    };
    let at = replace(doc, range, "").start;
    let rope = doc.text();
    let pos = rope.line_to_char(rope.char_to_line(at.min(rope.len_chars())));
    pos..pos
}

/// Moves the selected lines one line up or down. Returns `None` at the document edge.
pub fn move_lines(doc: &mut Document, selection: &Range<usize>, up: bool) -> Option<Range<usize>> {
    let lines = selected_lines(doc, selection);
    let (outer, neighbor) = if up {
        let above = lines.start.checked_sub(1)?;
        (above..lines.end, above)
    } else if lines.end < doc.text().len_lines() {
        (lines.start..lines.end + 1, lines.end)
    } else {
        return None;
    };
    let neighbor_len = doc
        .slice(block(doc, &(neighbor..neighbor + 1)))
        .chars()
        .count()
        + 1;
    let range = block(doc, &outer);
    let text = doc.slice(range.clone());
    let mut parts: Vec<&str> = text.split('\n').collect();
    if up {
        parts.rotate_left(1);
    } else {
        parts.rotate_right(1);
    }
    replace(doc, range, &parts.join("\n"));

    let shift = if up {
        -(neighbor_len as isize)
    } else {
        neighbor_len as isize
    };
    Some(selection.start.saturating_add_signed(shift)..selection.end.saturating_add_signed(shift))
}

pub fn sort_lines(
    doc: &mut Document,
    selection: &Range<usize>,
    order: SortOrder,
    descending: bool,
) -> Result<Range<usize>, SortError> {
    let selection = lines_or_all(doc, selection);
    let first_line = selected_lines(doc, &selection).start;
    let text = doc.slice(block(doc, &selected_lines(doc, &selection)));
    let mut lines: Vec<&str> = text.split('\n').collect();
    match order {
        SortOrder::Lexicographic => lines.sort(),
        SortOrder::IgnoreCase => lines.sort_by_cached_key(|line| line.to_lowercase()),
        SortOrder::Locale => lines.sort_by_cached_key(|line| locale_key(line)),
        SortOrder::Length => lines.sort_by_key(|line| line.chars().count()),
        SortOrder::Integer | SortOrder::DecimalComma | SortOrder::DecimalDot => {
            let mut keyed = Vec::with_capacity(lines.len());
            for (index, line) in lines.iter().enumerate() {
                let key = parse_number(line, order).ok_or(SortError {
                    line: first_line + index,
                })?;
                keyed.push((key, *line));
            }
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            lines = keyed.into_iter().map(|(_, line)| line).collect();
        }
    }
    if descending {
        lines.reverse();
    }
    Ok(map_lines(doc, &selection, |_| {
        lines.into_iter().map(str::to_owned).collect()
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Lexicographic,
    IgnoreCase,
    /// Letters before digits and punctuation, accents ignored first, then compared.
    Locale,
    Length,
    Integer,
    DecimalComma,
    DecimalDot,
}

/// A line that could not be parsed for numeric sorting (zero-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortError {
    pub line: usize,
}

fn locale_key(line: &str) -> (String, String) {
    let base: String = line
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ä' | 'à' | 'á' | 'â' | 'ã' | 'å' => 'a',
            'ö' | 'ò' | 'ó' | 'ô' | 'õ' | 'ø' => 'o',
            'ü' | 'ù' | 'ú' | 'û' => 'u',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ç' => 'c',
            'ñ' => 'n',
            'ß' => 's',
            c => c,
        })
        .collect();
    (base, line.to_owned())
}

fn parse_number(line: &str, order: SortOrder) -> Option<f64> {
    let text = line.trim();
    if text.is_empty() {
        return Some(f64::NEG_INFINITY);
    }
    match order {
        SortOrder::Integer => text.parse::<i128>().ok().map(|n| n as f64),
        SortOrder::DecimalComma => text.replace('.', "").replace(',', ".").parse().ok(),
        _ => text.replace(',', "").parse().ok(),
    }
    .filter(|n: &f64| !n.is_nan())
}

pub fn reverse_lines(doc: &mut Document, selection: &Range<usize>) -> Range<usize> {
    let selection = lines_or_all(doc, selection);
    map_lines(doc, &selection, |lines| {
        lines.into_iter().rev().map(str::to_owned).collect()
    })
}

/// Shuffles the lines with a small deterministic generator seeded by `seed`.
pub fn shuffle_lines(doc: &mut Document, selection: &Range<usize>, seed: u64) -> Range<usize> {
    let selection = lines_or_all(doc, selection);
    map_lines(doc, &selection, |mut lines| {
        let mut state = seed | 1;
        for i in (1..lines.len()).rev() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            lines.swap(i, (state % (i as u64 + 1)) as usize);
        }
        lines.into_iter().map(str::to_owned).collect()
    })
}

pub fn remove_consecutive_duplicates(doc: &mut Document, selection: &Range<usize>) -> Range<usize> {
    let selection = lines_or_all(doc, selection);
    map_lines(doc, &selection, |mut lines| {
        lines.dedup();
        lines.into_iter().map(str::to_owned).collect()
    })
}

/// Removes empty lines; with `blank`, also lines containing only whitespace.
pub fn remove_empty_lines(
    doc: &mut Document,
    selection: &Range<usize>,
    blank: bool,
) -> Range<usize> {
    let selection = lines_or_all(doc, selection);
    map_lines(doc, &selection, |lines| {
        lines
            .into_iter()
            .filter(|line| !(line.is_empty() || (blank && line.trim().is_empty())))
            .map(str::to_owned)
            .collect()
    })
}

/// Inserts an empty line above or below the cursor line and returns the position on it.
pub fn insert_blank_line(doc: &mut Document, cursor: usize, above: bool) -> usize {
    let rope = doc.text();
    let line = rope.char_to_line(cursor);
    if above {
        let start = rope.line_to_char(line);
        replace(doc, start..start, "\n");
        start
    } else {
        let end = block(doc, &(line..line + 1)).end;
        replace(doc, end..end, "\n").end
    }
}

/// Joins the selected lines (or the cursor line with the next) with single spaces.
pub fn join_lines(doc: &mut Document, selection: &Range<usize>) -> Range<usize> {
    let mut lines = selected_lines(doc, selection);
    if lines.len() == 1 {
        if lines.end >= doc.text().len_lines() {
            return selection.clone();
        }
        lines.end += 1;
    }
    let range = block(doc, &lines);
    let text = doc.slice(range.clone());
    let mut parts = text.split('\n');
    let mut joined = parts.next().unwrap_or_default().trim_end().to_owned();
    for part in parts.map(str::trim).filter(|part| !part.is_empty()) {
        joined.push(' ');
        joined.push_str(part);
    }
    replace(doc, range, &joined)
}

/// Breaks the selected lines at word boundaries so no line is wider than `width` chars.
pub fn split_lines(doc: &mut Document, selection: &Range<usize>, width: usize) -> Range<usize> {
    let width = width.max(1);
    map_lines(doc, selection, |lines| {
        let mut out = Vec::new();
        for line in lines {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            let mut current = String::new();
            for word in line.split_whitespace() {
                let len = current.chars().count();
                if len > 0 && len + 1 + word.chars().count() > width {
                    out.push(std::mem::take(&mut current));
                }
                if current.is_empty() {
                    current.push_str(&indent);
                } else {
                    current.push(' ');
                }
                current.push_str(word);
            }
            out.push(current);
        }
        out
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentMode {
    Toggle,
    Comment,
    Uncomment,
}

/// Comments out or uncomments the selected lines with a line comment `prefix`.
/// `Toggle` uncomments if all non-blank lines are commented.
pub fn comment_lines(
    doc: &mut Document,
    selection: &Range<usize>,
    prefix: &str,
    mode: CommentMode,
) -> Range<usize> {
    let marker = prefix.trim_end();
    map_lines(doc, selection, |lines| {
        let all_commented = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .all(|line| line.trim_start().starts_with(marker));
        let uncomment = match mode {
            CommentMode::Toggle => all_commented,
            CommentMode::Comment => false,
            CommentMode::Uncomment => true,
        };
        let indent = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        lines
            .into_iter()
            .map(|line| {
                let start = line.len() - line.trim_start().len();
                if line.trim().is_empty() {
                    line.to_owned()
                } else if uncomment {
                    match line[start..].strip_prefix(marker) {
                        Some(rest) => {
                            format!(
                                "{}{}",
                                &line[..start],
                                rest.strip_prefix(' ').unwrap_or(rest)
                            )
                        }
                        None => line.to_owned(),
                    }
                } else {
                    format!("{}{marker} {}", &line[..indent], &line[indent..])
                }
            })
            .collect()
    })
}

/// Wraps the selection (or the cursor line) in a block comment, or removes one around it.
pub fn block_comment(
    doc: &mut Document,
    selection: &Range<usize>,
    (open, close): (&str, &str),
    add: bool,
) -> Range<usize> {
    let range = if selection.is_empty() {
        block(doc, &selected_lines(doc, selection))
    } else {
        selection.clone()
    };
    let text = doc.slice(range.clone());
    let new = if add {
        format!("{open}{text}{close}")
    } else {
        let trimmed = text.trim();
        match trimmed
            .strip_prefix(open)
            .and_then(|t| t.strip_suffix(close))
        {
            Some(inner) => {
                let lead = &text[..text.len() - text.trim_start().len()];
                let trail = &text[text.trim_end().len()..];
                format!("{lead}{inner}{trail}")
            }
            None => return selection.clone(),
        }
    };
    replace(doc, range, &new)
}

pub fn remove_duplicate_lines(doc: &mut Document, selection: &Range<usize>) -> Range<usize> {
    let selection = lines_or_all(doc, selection);
    map_lines(doc, &selection, |lines| {
        let mut seen = HashSet::new();
        lines
            .into_iter()
            .filter(|line| seen.insert(*line))
            .map(str::to_owned)
            .collect()
    })
}

pub fn trim_trailing_whitespace(doc: &mut Document, selection: &Range<usize>) -> Range<usize> {
    let selection = lines_or_all(doc, selection);
    map_lines(doc, &selection, |lines| {
        lines.into_iter().map(|l| l.trim_end().to_owned()).collect()
    })
}

pub fn change_case(doc: &mut Document, selection: &Range<usize>, upper: bool) -> Range<usize> {
    if selection.is_empty() {
        return selection.clone();
    }
    let text = doc.slice(selection.clone());
    let new = if upper {
        text.to_uppercase()
    } else {
        text.to_lowercase()
    };
    if new == text {
        return selection.clone();
    }
    replace(doc, selection.clone(), &new)
}

pub fn indent_lines(doc: &mut Document, selection: &Range<usize>, unit: &str) -> Range<usize> {
    map_lines(doc, selection, |lines| {
        lines
            .into_iter()
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("{unit}{line}")
                }
            })
            .collect()
    })
}

pub fn unindent_lines(
    doc: &mut Document,
    selection: &Range<usize>,
    tab_width: usize,
) -> Range<usize> {
    map_lines(doc, selection, |lines| {
        lines
            .into_iter()
            .map(|line| {
                let remove = if line.starts_with('\t') {
                    1
                } else {
                    line.chars()
                        .take(tab_width)
                        .take_while(|&c| c == ' ')
                        .count()
                };
                line[remove..].to_owned()
            })
            .collect()
    })
}

/// Comments out the selected lines, or uncomments them if all are commented.
pub fn toggle_comment(doc: &mut Document, selection: &Range<usize>, prefix: &str) -> Range<usize> {
    comment_lines(doc, selection, prefix, CommentMode::Toggle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::new();
        doc.replace(0..0, text);
        doc
    }

    fn text(doc: &Document) -> String {
        doc.text().to_string()
    }

    #[test]
    fn finds_selected_lines() {
        let doc = doc("a\nb\nc");
        assert_eq!(selected_lines(&doc, &(0..0)), 0..1);
        assert_eq!(
            selected_lines(&doc, &(0..2)),
            0..1,
            "ends at start of line 1"
        );
        assert_eq!(selected_lines(&doc, &(1..3)), 0..2);
        assert_eq!(selected_lines(&doc, &(5..5)), 2..3);
    }

    #[test]
    fn duplicates_and_deletes() {
        let mut d = doc("a\nb\nc");
        assert_eq!(duplicate_lines(&mut d, &(2..2)), 4..5);
        assert_eq!(text(&d), "a\nb\nb\nc");
        d.undo();
        assert_eq!(text(&d), "a\nb\nc");

        let mut d = doc("a\nb\nc");
        delete_lines(&mut d, &(2..3));
        assert_eq!(text(&d), "a\nc");
        let mut d = doc("a\nb\nc");
        delete_lines(&mut d, &(4..5));
        assert_eq!(text(&d), "a\nb");
        let mut d = doc("only");
        delete_lines(&mut d, &(1..1));
        assert_eq!(text(&d), "");
    }

    #[test]
    fn moves_lines() {
        let mut d = doc("a\nbb\nc");
        assert_eq!(move_lines(&mut d, &(3..3), true), Some(1..1));
        assert_eq!(text(&d), "bb\na\nc");
        assert_eq!(move_lines(&mut d, &(1..1), true), None);
        assert_eq!(move_lines(&mut d, &(1..1), false), Some(3..3));
        assert_eq!(text(&d), "a\nbb\nc");
        assert_eq!(move_lines(&mut d, &(6..6), false), None);
    }

    #[test]
    fn sorts_dedups_and_trims() {
        let mut d = doc("b\na\nb  \nc");
        trim_trailing_whitespace(&mut d, &(0..0));
        assert_eq!(text(&d), "b\na\nb\nc");
        remove_duplicate_lines(&mut d, &(0..0));
        assert_eq!(text(&d), "b\na\nc");
        sort_lines(&mut d, &(0..0), SortOrder::Lexicographic, false).unwrap();
        assert_eq!(text(&d), "a\nb\nc");
        sort_lines(&mut d, &(0..3), SortOrder::Lexicographic, true).unwrap();
        assert_eq!(text(&d), "b\na\nc", "only the two selected lines");
    }

    #[test]
    fn sorts_in_every_order() {
        let sorted = |input: &str, order: SortOrder| {
            let mut d = doc(input);
            sort_lines(&mut d, &(0..0), order, false).map(|_| text(&d))
        };
        assert_eq!(
            sorted("b\nA\na\nB", SortOrder::Lexicographic),
            Ok("A\nB\na\nb".into())
        );
        assert_eq!(
            sorted("b\nA\nC", SortOrder::IgnoreCase),
            Ok("A\nb\nC".into())
        );
        assert_eq!(
            sorted("äb\nz\naa", SortOrder::Locale),
            Ok("aa\näb\nz".into())
        );
        assert_eq!(
            sorted("ccc\na\nbb", SortOrder::Length),
            Ok("a\nbb\nccc".into())
        );
        assert_eq!(
            sorted("10\n9\n-1", SortOrder::Integer),
            Ok("-1\n9\n10".into())
        );
        assert_eq!(
            sorted("1,5\n1.000,2\n0,9", SortOrder::DecimalComma),
            Ok("0,9\n1,5\n1.000,2".into())
        );
        assert_eq!(
            sorted("1.5\n1,000.2\n0.9", SortOrder::DecimalDot),
            Ok("0.9\n1.5\n1,000.2".into())
        );
        assert_eq!(
            sorted("1\nx\n2", SortOrder::Integer),
            Err(SortError { line: 1 })
        );
    }

    #[test]
    fn reorders_and_cleans_lines() {
        let mut d = doc("a\na\nb\na");
        remove_consecutive_duplicates(&mut d, &(0..0));
        assert_eq!(text(&d), "a\nb\na");
        reverse_lines(&mut d, &(0..0));
        assert_eq!(text(&d), "a\nb\na");

        let mut d = doc("1\n2\n3\n4\n5");
        shuffle_lines(&mut d, &(0..0), 42);
        let mut lines: Vec<String> = text(&d).split('\n').map(str::to_owned).collect();
        lines.sort();
        assert_eq!(lines, ["1", "2", "3", "4", "5"]);

        let mut d = doc("a\n\n  \nb");
        remove_empty_lines(&mut d, &(0..0), false);
        assert_eq!(text(&d), "a\n  \nb");
        remove_empty_lines(&mut d, &(0..0), true);
        assert_eq!(text(&d), "a\nb");
    }

    #[test]
    fn inserts_joins_and_splits() {
        let mut d = doc("a\nb");
        assert_eq!(insert_blank_line(&mut d, 0, false), 2);
        assert_eq!(text(&d), "a\n\nb");
        assert_eq!(insert_blank_line(&mut d, 3, true), 3);
        assert_eq!(text(&d), "a\n\n\nb");

        let mut d = doc("one \n  two\nthree");
        join_lines(&mut d, &(0..0));
        assert_eq!(text(&d), "one two\nthree");

        let mut d = doc("  aa bb cc dd");
        split_lines(&mut d, &(0..0), 7);
        assert_eq!(text(&d), "  aa bb\n  cc dd");
    }

    #[test]
    fn comments_explicitly_and_in_blocks() {
        let mut d = doc("// a\nb");
        let all = 0..d.text().len_chars();
        comment_lines(&mut d, &all, "//", CommentMode::Uncomment);
        assert_eq!(text(&d), "a\nb");
        let all = 0..d.text().len_chars();
        comment_lines(&mut d, &all, "//", CommentMode::Comment);
        assert_eq!(text(&d), "// a\n// b");

        let mut d = doc("x = 1;");
        let range = block_comment(&mut d, &(4..5), ("/*", "*/"), true);
        assert_eq!(text(&d), "x = /*1*/;");
        block_comment(&mut d, &range, ("/*", "*/"), false);
        assert_eq!(text(&d), "x = 1;");
        block_comment(&mut d, &(0..0), ("/*", "*/"), true);
        assert_eq!(text(&d), "/*x = 1;*/");
    }

    #[test]
    fn retains_lines() {
        let mut d = doc("a\nb\nc\nd");
        assert_eq!(retain_lines(&mut d, |line| line % 2 == 0), 2);
        assert_eq!(text(&d), "a\nc");
        d.undo();
        assert_eq!(text(&d), "a\nb\nc\nd");
        assert_eq!(retain_lines(&mut d, |_| true), 0);
    }

    #[test]
    fn changes_case() {
        let mut d = doc("Grüße");
        assert_eq!(change_case(&mut d, &(0..5), true), 0..6);
        assert_eq!(text(&d), "GRÜSSE");
    }

    #[test]
    fn indents_and_comments() {
        let mut d = doc("a\n\n  b");
        let all = 0..d.text().len_chars();
        let sel = indent_lines(&mut d, &all, "\t");
        assert_eq!(text(&d), "\ta\n\n\t  b");
        unindent_lines(&mut d, &sel, 4);
        assert_eq!(text(&d), "a\n\n  b");

        let mut d = doc("  x\n    y");
        let all = 0..d.text().len_chars();
        let sel = toggle_comment(&mut d, &all, "//");
        assert_eq!(text(&d), "  // x\n  //   y");
        toggle_comment(&mut d, &sel, "//");
        assert_eq!(text(&d), "  x\n    y");
    }
}
