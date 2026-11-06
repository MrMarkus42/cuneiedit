use std::ops::Range;

use pilcrow_core::{Rope, RopeSlice};

pub(crate) fn line_len(rope: &Rope, line: usize) -> usize {
    let slice = rope.line(line);
    let len = slice.len_chars();
    if len > 0 && slice.char(len - 1) == '\n' {
        len - 1
    } else {
        len
    }
}

/// Zero-based line and column (in chars) of `pos`.
pub(crate) fn line_col(rope: &Rope, pos: usize) -> (usize, usize) {
    let line = rope.char_to_line(pos);
    (line, pos - rope.line_to_char(line))
}

pub(crate) fn line_end(rope: &Rope, line: usize) -> usize {
    rope.line_to_char(line) + line_len(rope, line)
}

/// Position of the first non-blank char of `line`, or the line start if the cursor is already there.
pub(crate) fn smart_home(rope: &Rope, pos: usize) -> usize {
    let (line, col) = line_col(rope, pos);
    let start = rope.line_to_char(line);
    let indent = leading_whitespace(rope.line(line)).chars().count();
    if col == indent { start } else { start + indent }
}

pub(crate) fn leading_whitespace(line: RopeSlice<'_>) -> String {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Newline,
    Space,
    Word,
    Punct,
}

fn class(ch: char) -> CharClass {
    if ch == '\n' {
        CharClass::Newline
    } else if ch.is_whitespace() {
        CharClass::Space
    } else if ch.is_alphanumeric() || ch == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

fn is_blank(ch: char) -> bool {
    matches!(class(ch), CharClass::Space | CharClass::Newline)
}

pub(crate) fn prev_word(rope: &Rope, mut pos: usize) -> usize {
    while pos > 0 && is_blank(rope.char(pos - 1)) {
        pos -= 1;
    }
    if pos > 0 {
        let kind = class(rope.char(pos - 1));
        while pos > 0 && class(rope.char(pos - 1)) == kind {
            pos -= 1;
        }
    }
    pos
}

pub(crate) fn next_word(rope: &Rope, mut pos: usize) -> usize {
    let len = rope.len_chars();
    while pos < len && is_blank(rope.char(pos)) {
        pos += 1;
    }
    if pos < len {
        let kind = class(rope.char(pos));
        while pos < len && class(rope.char(pos)) == kind {
            pos += 1;
        }
    }
    pos
}

/// Range of the run of same-class chars at `pos`, as selected by a double click.
pub(crate) fn word_at(rope: &Rope, pos: usize) -> Range<usize> {
    let len = rope.len_chars();
    let kind = if pos < len {
        class(rope.char(pos))
    } else if pos > 0 {
        class(rope.char(pos - 1))
    } else {
        return pos..pos;
    };
    if kind == CharClass::Newline {
        return pos..pos;
    }
    let (mut start, mut end) = (pos, pos);
    while start > 0 && class(rope.char(start - 1)) == kind {
        start -= 1;
    }
    while end < len && class(rope.char(end)) == kind {
        end += 1;
    }
    start..end
}

/// Display columns `c` takes at display column `display`: tabs reach the next tab stop,
/// control characters are shown in caret notation (`^A`).
pub(crate) fn char_columns(c: char, display: usize, tab_width: usize) -> usize {
    match c {
        '\t' => tab_width - display % tab_width,
        c if is_control(c) => 2,
        _ => 1,
    }
}

fn is_control(c: char) -> bool {
    (c < ' ' && c != '\t' && c != '\n') || c == '\u{7f}'
}

/// Expands tabs to spaces for text starting at display column `start`. Returns the display text
/// and, for every char (plus one past the end), its char index in the display text.
pub(crate) fn expand_tabs(
    chars: impl Iterator<Item = char>,
    tab_width: usize,
    start: usize,
) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut columns = Vec::new();
    let mut display = 0;
    for ch in chars {
        if ch == '\n' {
            break;
        }
        columns.push(display);
        let width = char_columns(ch, start + display, tab_width);
        if ch == '\t' {
            text.extend(std::iter::repeat_n(' ', width));
        } else if is_control(ch) {
            text.push('^');
            text.push(char::from(ch as u8 ^ 0x40));
        } else {
            text.push(ch);
        }
        display += width;
    }
    columns.push(display);
    (text, columns)
}

/// First column of `line` whose display column is at least `target`, with its display column.
pub(crate) fn column_at_display(
    line: RopeSlice<'_>,
    target: usize,
    tab_width: usize,
) -> (usize, usize) {
    let mut display = 0;
    for (col, ch) in line.chars().enumerate() {
        if display >= target || ch == '\n' {
            return (col, display);
        }
        display += char_columns(ch, display, tab_width);
    }
    (line.len_chars(), display)
}

/// Display column of char column `col` in `line`.
pub(crate) fn display_column(line: RopeSlice<'_>, col: usize, tab_width: usize) -> usize {
    line.chars()
        .take(col)
        .take_while(|&c| c != '\n')
        .fold(0, |display, c| {
            display + char_columns(c, display, tab_width)
        })
}

/// Char columns where the visual rows of a line start when wrapped at `width` display columns.
/// Breaks after the last space that fits; words longer than a row are split. Always starts
/// with 0.
pub(crate) fn wrap_points(
    chars: impl Iterator<Item = char>,
    tab_width: usize,
    width: usize,
) -> Vec<usize> {
    let width = width.max(1);
    let mut points = vec![0];
    let (mut display, mut row_start_display) = (0, 0);
    // Char column and display column just after the last space in the current row.
    let mut last_break: Option<(usize, usize)> = None;
    for (col, ch) in chars.enumerate() {
        if ch == '\n' {
            break;
        }
        let columns = char_columns(ch, display, tab_width);
        let row_start = *points.last().expect("starts with 0");
        let blank = ch == ' ' || ch == '\t';
        // Trailing blanks may hang past the edge instead of starting a row.
        if !blank && display + columns - row_start_display > width && col > row_start {
            let (break_col, break_display) = last_break
                .filter(|&(c, _)| c > row_start)
                .unwrap_or((col, display));
            points.push(break_col);
            row_start_display = break_display;
            last_break = None;
        }
        display += columns;
        if blank {
            last_break = Some((col + 1, display));
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_lines() {
        let rope = Rope::from_str("ab\n\ncd");
        assert_eq!(rope.len_lines(), 3);
        assert_eq!(line_len(&rope, 0), 2);
        assert_eq!(line_len(&rope, 1), 0);
        assert_eq!(line_len(&rope, 2), 2);
        assert_eq!(line_col(&rope, 5), (2, 1));
        assert_eq!(line_end(&rope, 0), 2);
    }

    #[test]
    fn only_newline_breaks_lines() {
        let rope = Rope::from_str("a\u{2028}b\u{85}c\rd");
        assert_eq!(rope.len_lines(), 1);
    }

    #[test]
    fn jumps_words() {
        let rope = Rope::from_str("foo.bar  baz\nqux");
        assert_eq!(next_word(&rope, 0), 3);
        assert_eq!(next_word(&rope, 3), 4);
        assert_eq!(next_word(&rope, 7), 12);
        assert_eq!(next_word(&rope, 12), 16);
        assert_eq!(prev_word(&rope, 16), 13);
        assert_eq!(prev_word(&rope, 13), 9);
        assert_eq!(prev_word(&rope, 4), 3);
    }

    #[test]
    fn selects_word_at() {
        let rope = Rope::from_str("let x_1 = 42;\n");
        assert_eq!(word_at(&rope, 5), 4..7);
        assert_eq!(word_at(&rope, 3), 3..4);
        assert_eq!(word_at(&rope, 13), 13..13);
    }

    #[test]
    fn smart_home_toggles() {
        let rope = Rope::from_str("x\n    foo");
        assert_eq!(smart_home(&rope, 9), 6);
        assert_eq!(smart_home(&rope, 6), 2);
    }

    #[test]
    fn expands_tabs_to_stops() {
        let rope = Rope::from_str("a\tbc\td\n");
        let (text, columns) = expand_tabs(rope.line(0).chars(), 4, 0);
        assert_eq!(text, "a   bc  d");
        assert_eq!(columns, vec![0, 1, 4, 5, 6, 8, 9]);
    }

    #[test]
    fn expands_tabs_from_offset() {
        let (text, columns) = expand_tabs("\tx".chars(), 4, 3);
        assert_eq!(text, " x");
        assert_eq!(columns, vec![0, 1, 2]);
    }

    #[test]
    fn shows_control_chars_in_caret_notation() {
        let (text, columns) = expand_tabs("a\u{1}b\u{7f}".chars(), 4, 0);
        assert_eq!(text, "a^Ab^?");
        assert_eq!(columns, vec![0, 1, 3, 4, 6]);
        assert_eq!(display_column(Rope::from_str("\ta\u{0}b").line(0), 3, 4), 7);
    }

    #[test]
    fn wraps_at_word_boundaries() {
        let points = |text: &str, width| wrap_points(text.chars(), 4, width);
        assert_eq!(points("short", 10), vec![0]);
        assert_eq!(points("aaa bbb ccc", 7), vec![0, 8]);
        assert_eq!(points("aaaaaaaaaa", 4), vec![0, 4, 8]);
        assert_eq!(points("ab cdefghij", 4), vec![0, 3, 7]);
        assert_eq!(points("\tab cd", 6), vec![0, 4]);
        assert_eq!(points("", 4), vec![0]);
    }

    #[test]
    fn finds_column_at_display() {
        let rope = Rope::from_str("\tab\n");
        assert_eq!(column_at_display(rope.line(0), 0, 4), (0, 0));
        assert_eq!(column_at_display(rope.line(0), 2, 4), (1, 4));
        assert_eq!(column_at_display(rope.line(0), 5, 4), (2, 5));
        assert_eq!(column_at_display(rope.line(0), 99, 4), (3, 6));
    }
}
