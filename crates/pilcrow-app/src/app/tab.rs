//! One open document with its view state.

use std::ops::Range;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::Color32;
use pilcrow_core::diff::{changed_lines, line_hashes};
use pilcrow_core::highlight::{Highlighter, Language as Syntax};
use pilcrow_core::ops::{self, SortError};
use pilcrow_core::transform::{self, Case};
use pilcrow_core::{Document, Search, SearchQuery};
use pilcrow_editor::EditorState;

use super::command::LineOp;
use crate::i18n::{Texts, fill};
use crate::settings::{Flag, Settings};

pub const MARK_STYLES: usize = 5;
/// Change history is not tracked for longer documents, to keep typing fast.
const MAX_HISTORY_LINES: usize = 100_000;

type ColoredRanges = Vec<(Range<usize>, Color32)>;

pub struct Tab {
    pub id: u64,
    pub doc: Document,
    pub editor: EditorState,
    pub split: Option<Split>,
    pub highlighter: Highlighter,
    pub untitled_number: usize,
    pub read_only: bool,
    pub pinned: bool,
    pub color: Option<usize>,
    /// Reload automatically when the file changes and keep the end in view.
    pub monitoring: bool,
    pub marks: Marks,
    pub history: ChangeHistory,
    /// Revision whose text is in the backup file.
    pub backed_up: Option<u64>,
}

/// A second view of the same document. `Tab::editor` is always the focused view.
pub struct Split {
    pub other: EditorState,
    pub main_on_left: bool,
}

impl Tab {
    pub fn new(id: u64, doc: Document, untitled_number: usize) -> Self {
        let syntax = doc.path().map_or(Syntax::PLAIN_TEXT, Syntax::from_path);
        let mut history = ChangeHistory::default();
        history.reset(&doc);
        Self {
            id,
            doc,
            editor: EditorState::default(),
            split: None,
            highlighter: Highlighter::new(syntax),
            untitled_number,
            read_only: false,
            pinned: false,
            color: None,
            monitoring: false,
            marks: Marks::default(),
            history,
            backed_up: None,
        }
    }

    pub fn title(&self, texts: &Texts) -> String {
        self.doc
            .file_name()
            .unwrap_or_else(|| fill(texts.untitled, &[("n", &self.untitled_number.to_string())]))
    }

    pub fn is_pristine(&self) -> bool {
        self.doc.path().is_none() && !self.doc.is_modified() && self.doc.text().len_chars() == 0
    }

    pub fn syntax(&self) -> Syntax {
        self.highlighter.language()
    }

    /// Applies a line operation; numeric sorting fails on lines without a number.
    pub fn apply_line_op(&mut self, op: LineOp, settings: &Settings) -> Result<(), SortError> {
        let Self {
            doc,
            editor,
            highlighter,
            ..
        } = self;
        let selection = editor.selection();
        let len_before = doc.text().len_chars();
        let tab_width = settings.tab_width;
        let language = highlighter.language();
        let result = match op {
            LineOp::Duplicate => {
                let copy = ops::duplicate_lines(doc, &selection);
                let shift = copy.len() + 1;
                Some(selection.start + shift..selection.end + shift)
            }
            LineOp::Delete => Some(ops::delete_lines(doc, &selection)),
            LineOp::MoveUp => ops::move_lines(doc, &selection, true),
            LineOp::MoveDown => ops::move_lines(doc, &selection, false),
            LineOp::RemoveDuplicates => Some(ops::remove_duplicate_lines(doc, &selection)),
            LineOp::RemoveConsecutiveDuplicates => {
                Some(ops::remove_consecutive_duplicates(doc, &selection))
            }
            LineOp::Split => Some(ops::split_lines(doc, &selection, settings.edge_column)),
            LineOp::Join => Some(ops::join_lines(doc, &selection)),
            LineOp::BlankAbove | LineOp::BlankBelow => {
                let cursor = editor.cursor();
                let pos = ops::insert_blank_line(doc, cursor, op == LineOp::BlankAbove);
                Some(pos..pos)
            }
            LineOp::RemoveEmpty => Some(ops::remove_empty_lines(doc, &selection, false)),
            LineOp::RemoveBlank => Some(ops::remove_empty_lines(doc, &selection, true)),
            LineOp::Reverse => Some(ops::reverse_lines(doc, &selection)),
            LineOp::Shuffle => Some(ops::shuffle_lines(doc, &selection, seed())),
            LineOp::Sort(order, descending) => {
                Some(ops::sort_lines(doc, &selection, order, descending)?)
            }
            LineOp::Indent => {
                let unit = if settings.get(Flag::IndentWithSpaces) {
                    " ".repeat(tab_width)
                } else {
                    "\t".to_owned()
                };
                Some(ops::indent_lines(doc, &selection, &unit))
            }
            LineOp::Unindent => Some(ops::unindent_lines(doc, &selection, tab_width)),
            LineOp::TrimTrailing => Some(ops::trim_trailing_whitespace(doc, &selection)),
            LineOp::Case(case) => {
                let case = match case {
                    Case::Random(_) => Case::Random(seed()),
                    case => case,
                };
                Some(transform::change_case(doc, &selection, case))
            }
            LineOp::Blank(blank) => Some(transform::blank_operation(
                doc, &selection, blank, tab_width,
            )),
            LineOp::Comment(mode) => language
                .line_comment()
                .map(|prefix| ops::comment_lines(doc, &selection, prefix, mode)),
            LineOp::BlockComment(add) => language
                .block_comment()
                .map(|pair| ops::block_comment(doc, &selection, pair, add)),
            LineOp::Redact => Some(transform::redact(doc, &selection, '█')),
        };
        let Some(result) = result else {
            return Ok(());
        };
        let len = doc.text().len_chars();
        let stays_on_line = matches!(
            op,
            LineOp::Comment(_) | LineOp::Indent | LineOp::Unindent | LineOp::BlockComment(_)
        );
        let keeps_cursor = !matches!(
            op,
            LineOp::Duplicate
                | LineOp::Delete
                | LineOp::MoveUp
                | LineOp::MoveDown
                | LineOp::BlankAbove
                | LineOp::BlankBelow
        );
        if matches!(op, LineOp::BlankAbove | LineOp::BlankBelow) {
            editor.set_cursor(result.start);
        } else if keeps_cursor && selection.is_empty() {
            let cursor = if stays_on_line {
                // Keep the cursor next to the same text, shifted by what was added before it.
                let delta = len as isize - len_before as isize;
                let line_start = doc
                    .text()
                    .line_to_char(doc.text().char_to_line(selection.start.min(len)));
                selection.start.saturating_add_signed(delta).max(line_start)
            } else {
                selection.start
            };
            editor.set_cursor(cursor.min(len));
        } else {
            editor.set_selection(result.start.min(len), result.end.min(len));
        }
        Ok(())
    }

    /// Whether `op` can be applied to this document.
    pub fn supports(&self, op: LineOp) -> bool {
        match op {
            LineOp::Comment(_) => self.syntax().line_comment().is_some(),
            LineOp::BlockComment(_) => self.syntax().block_comment().is_some(),
            LineOp::Redact | LineOp::Case(_) => !self.editor.selection().is_empty(),
            _ => true,
        }
    }
}

/// Seed for random operations.
fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

/// Up to five highlight styles, each marking all occurrences of a text.
#[derive(Default)]
pub struct Marks {
    queries: [Option<SearchQuery>; MARK_STYLES],
    /// Document revision and the highlighted ranges.
    cache: Option<(u64, ColoredRanges)>,
}

impl Marks {
    pub fn set(&mut self, style: usize, text: Option<String>) {
        self.queries[style] = text.map(|pattern| SearchQuery {
            pattern,
            match_case: true,
            ..SearchQuery::default()
        });
        self.cache = None;
    }

    pub fn clear_all(&mut self) {
        *self = Self::default();
    }

    pub fn is_set(&self, style: usize) -> bool {
        self.queries[style].is_some()
    }

    pub fn any(&self) -> bool {
        self.queries.iter().any(Option::is_some)
    }

    pub fn spans(&mut self, doc: &Document) -> &[(Range<usize>, Color32)] {
        if !self.any() {
            return &[];
        }
        if self
            .cache
            .as_ref()
            .is_none_or(|(rev, _)| *rev != doc.revision())
        {
            let mut spans = Vec::new();
            for (style, query) in self.queries.iter().enumerate() {
                let Some(Ok(Some(search))) = query.as_ref().map(Search::new) else {
                    continue;
                };
                let color = mark_color(style);
                spans.extend(search.find_all(doc).into_iter().map(|r| (r, color)));
            }
            spans.sort_by_key(|(range, _)| range.start);
            self.cache = Some((doc.revision(), spans));
        }
        self.cache.as_ref().map_or(&[], |(_, spans)| spans)
    }
}

pub fn mark_color(style: usize) -> Color32 {
    const COLORS: [(u8, u8, u8); MARK_STYLES] = [
        (0, 200, 255),
        (255, 128, 0),
        (255, 220, 0),
        (140, 70, 255),
        (0, 190, 80),
    ];
    let (r, g, b) = COLORS[style % MARK_STYLES];
    Color32::from_rgba_unmultiplied(r, g, b, 110)
}

/// Lines changed since the file was opened, as gutter marks: orange if unsaved,
/// green if saved since.
#[derive(Default)]
pub struct ChangeHistory {
    opened: Option<Vec<u64>>,
    saved: Option<Vec<u64>>,
    revision: Option<u64>,
    marks: Vec<(Range<usize>, Color32)>,
}

impl ChangeHistory {
    /// Starts over with `doc` as the unchanged state, e.g. after opening or reloading.
    pub fn reset(&mut self, doc: &Document) {
        let hashes = (doc.text().len_lines() <= MAX_HISTORY_LINES).then(|| line_hashes(doc));
        *self = Self {
            opened: hashes.clone(),
            saved: hashes,
            ..Self::default()
        };
    }

    pub fn mark_saved(&mut self, doc: &Document) {
        if self.opened.is_some() && doc.text().len_lines() <= MAX_HISTORY_LINES {
            self.saved = Some(line_hashes(doc));
            self.revision = None;
        }
    }

    pub fn marks(&mut self, doc: &Document) -> &[(Range<usize>, Color32)] {
        if self.revision == Some(doc.revision()) {
            return &self.marks;
        }
        self.revision = Some(doc.revision());
        self.marks.clear();
        let (Some(opened), Some(saved)) = (&self.opened, &self.saved) else {
            return &self.marks;
        };
        if doc.text().len_lines() > MAX_HISTORY_LINES {
            return &self.marks;
        }
        let current = line_hashes(doc);
        let saved_color = Color32::from_rgb(60, 170, 90);
        let unsaved_color = Color32::from_rgb(255, 140, 0);
        self.marks.extend(
            changed_lines(opened, &current)
                .into_iter()
                .map(|lines| (lines, saved_color)),
        );
        self.marks.extend(
            changed_lines(saved, &current)
                .into_iter()
                .map(|lines| (lines, unsaved_color)),
        );
        &self.marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pilcrow_core::ops::CommentMode;

    fn tab(text: &str) -> Tab {
        let mut doc = Document::new();
        doc.replace(0..0, text);
        Tab::new(1, doc, 1)
    }

    #[test]
    fn tracks_changes_since_open_and_save() {
        let mut tab = tab("a\nb\nc");
        tab.history.reset(&tab.doc);
        assert!(tab.history.marks(&tab.doc).is_empty());
        tab.doc.replace(2..3, "B");
        let unsaved = tab.history.marks(&tab.doc).to_vec();
        assert_eq!(unsaved.len(), 2, "changed since open and since save");
        tab.history.mark_saved(&tab.doc);
        let marks = tab.history.marks(&tab.doc).to_vec();
        assert_eq!(marks, vec![(1..2, Color32::from_rgb(60, 170, 90))]);
    }

    #[test]
    fn marks_all_occurrences() {
        let mut tab = tab("foo bar foo");
        tab.marks.set(0, Some("foo".into()));
        tab.marks.set(2, Some("bar".into()));
        let spans: Vec<_> = tab
            .marks
            .spans(&tab.doc)
            .iter()
            .map(|s| s.0.clone())
            .collect();
        assert_eq!(spans, vec![0..3, 4..7, 8..11]);
        tab.marks.set(0, None);
        assert_eq!(tab.marks.spans(&tab.doc).len(), 1);
    }

    #[test]
    fn comments_keep_the_cursor_on_its_text() {
        let mut tab = tab("let x = 1;");
        tab.highlighter
            .set_language(Syntax::from_name("Rust").unwrap());
        tab.editor.set_cursor(4);
        let settings = Settings::default();
        tab.apply_line_op(LineOp::Comment(CommentMode::Toggle), &settings)
            .unwrap();
        assert_eq!(tab.doc.text().to_string(), "// let x = 1;");
        assert_eq!(tab.editor.cursor(), 7);
    }

    #[test]
    fn reports_lines_that_cannot_be_sorted_numerically() {
        let mut tab = tab("2\nx\n1");
        let settings = Settings::default();
        let order = pilcrow_core::ops::SortOrder::Integer;
        let err = tab
            .apply_line_op(LineOp::Sort(order, false), &settings)
            .unwrap_err();
        assert_eq!(err.line, 1);
    }
}
