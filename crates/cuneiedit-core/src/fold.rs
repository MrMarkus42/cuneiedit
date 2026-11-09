//! Code folding: which lines start a foldable block and which lines it hides.
//!
//! A line starts a block if it opens a bracket that closes on a later line, or if the next
//! non-blank line is indented deeper. This works for brace languages and for indentation-based
//! ones (Python, YAML) without a parser.

use std::ops::Range;

use ropey::Rope;

use crate::transform::matching_bracket;

/// How far to look for the next non-blank line when checking indentation.
const MAX_BLANK_LOOKAHEAD: usize = 50;
/// How far "fold current block" looks upwards for the enclosing block.
const MAX_ENCLOSING_SCAN: usize = 10_000;

/// Indentation of `line` in display columns, or `None` if it is blank.
fn indentation(rope: &Rope, line: usize, tab_width: usize) -> Option<usize> {
    let mut width = 0;
    for c in rope.line(line).chars() {
        match c {
            ' ' => width += 1,
            '\t' => width += tab_width - width % tab_width,
            '\n' | '\r' => return None,
            _ => return Some(width),
        }
    }
    None
}

/// Char position of the last bracket on `line` that is not closed on the same line.
/// Text in simple quotes is skipped.
fn unclosed_bracket(rope: &Rope, line: usize) -> Option<usize> {
    let start = rope.line_to_char(line);
    let mut open = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    for (offset, c) in rope.line(line).chars().enumerate() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => quote = Some(c),
            '{' | '[' | '(' => open.push(start + offset),
            '}' | ']' | ')' => {
                open.pop();
            }
            _ => {}
        }
    }
    open.last().copied()
}

fn next_non_blank(rope: &Rope, line: usize, tab_width: usize) -> Option<(usize, usize)> {
    (line + 1..rope.len_lines())
        .take(MAX_BLANK_LOOKAHEAD)
        .find_map(|l| indentation(rope, l, tab_width).map(|indent| (l, indent)))
}

/// Whether `line` starts a foldable block. Cheap: looks at this line and the next few.
pub fn is_header(rope: &Rope, line: usize, tab_width: usize) -> bool {
    if line + 1 >= rope.len_lines() {
        return false;
    }
    if unclosed_bracket(rope, line).is_some() {
        return true;
    }
    let Some(indent) = indentation(rope, line, tab_width) else {
        return false;
    };
    next_non_blank(rope, line, tab_width).is_some_and(|(_, next)| next > indent)
}

/// Lines hidden when folding the block that starts at `line`.
pub fn fold_range(rope: &Rope, line: usize, tab_width: usize) -> Option<Range<usize>> {
    if let Some(open) = unclosed_bracket(rope, line)
        && let Some((_, close)) = matching_bracket(rope, open)
    {
        let close_line = rope.char_to_line(close);
        let closing_first = rope
            .line(close_line)
            .chars()
            .find(|c| !c.is_whitespace())
            .is_some_and(|c| rope.char(close) == c);
        // A line starting with the closing bracket stays visible, e.g. `}` or `} else {`.
        let end = if closing_first {
            close_line
        } else {
            close_line + 1
        };
        if end > line + 1 {
            return Some(line + 1..end);
        }
    }
    let indent = indentation(rope, line, tab_width)?;
    let (first, next) = next_non_blank(rope, line, tab_width)?;
    if next <= indent {
        return None;
    }
    let mut last = first;
    for l in first + 1..rope.len_lines() {
        match indentation(rope, l, tab_width) {
            Some(i) if i <= indent => break,
            Some(_) => last = l,
            None => {}
        }
    }
    Some(line + 1..last + 1)
}

/// The outermost blocks, as `(header line, hidden lines)`, e.g. for "fold all".
pub fn top_level_folds(rope: &Rope, tab_width: usize) -> Vec<(usize, usize)> {
    let mut folds = Vec::new();
    let mut line = 0;
    while line < rope.len_lines() {
        if is_header(rope, line, tab_width)
            && let Some(range) = fold_range(rope, line, tab_width)
        {
            folds.push((line, range.len()));
            line = range.end;
        } else {
            line += 1;
        }
    }
    folds
}

/// The header of the innermost block that contains `line` or starts at it.
pub fn enclosing_header(rope: &Rope, line: usize, tab_width: usize) -> Option<(usize, usize)> {
    (line.saturating_sub(MAX_ENCLOSING_SCAN)..=line)
        .rev()
        .filter(|&l| is_header(rope, l, tab_width))
        .find_map(|l| {
            let range = fold_range(rope, l, tab_width)?;
            (l == line || range.contains(&line)).then_some((l, range.len()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_bracket_blocks() {
        let rope = Rope::from_str("fn a() {\n    x();\n} else {\n    y(\"{\");\n}\nz\n");
        assert!(is_header(&rope, 0, 4));
        assert!(!is_header(&rope, 1, 4));
        assert_eq!(fold_range(&rope, 0, 4), Some(1..2));
        assert_eq!(fold_range(&rope, 2, 4), Some(3..4));
        assert!(!is_header(&rope, 3, 4), "brackets in strings are ignored");
        assert_eq!(top_level_folds(&rope, 4), vec![(0, 1), (2, 1)]);
        assert_eq!(enclosing_header(&rope, 3, 4), Some((2, 1)));
    }

    #[test]
    fn folds_closing_lines_with_other_text() {
        let rope = Rope::from_str("call(a,\n  b,\n  c)\nnext\n");
        assert_eq!(fold_range(&rope, 0, 4), Some(1..3));
    }

    #[test]
    fn folds_by_indentation() {
        let rope = Rope::from_str("def f():\n    a\n\n    b\n\nc\n");
        assert!(is_header(&rope, 0, 4));
        assert_eq!(fold_range(&rope, 0, 4), Some(1..4));
        assert!(!is_header(&rope, 1, 4));
        assert_eq!(enclosing_header(&rope, 3, 4), Some((0, 3)));
        assert_eq!(enclosing_header(&rope, 5, 4), None);
    }
}
