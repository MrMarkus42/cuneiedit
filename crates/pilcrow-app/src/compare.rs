//! Side-by-side comparison of two documents, like Notepad++'s ComparePlus plugin.

use std::ops::Range;

use eframe::egui::{Color32, Vec2};
use pilcrow_core::Document;
use pilcrow_core::diff::{self, Comparison, DiffOptions, HunkKind};
use pilcrow_core::highlight::Highlighter;
use pilcrow_editor::EditorState;

/// The saved file on disk, shown read-only next to its open tab.
pub struct SavedCopy {
    pub doc: Document,
    pub editor: EditorState,
    pub highlighter: Highlighter,
    pub title: String,
}

pub enum Side {
    Tab(u64),
    Saved(Box<SavedCopy>),
}

#[derive(Default)]
pub struct Decorations {
    pub backgrounds: Vec<(Range<usize>, Color32)>,
    pub spans: Vec<(Range<usize>, Color32)>,
    pub spacers: Vec<(usize, usize)>,
}

/// Above this many lines, edits are only re-compared after a pause in typing.
const DEBOUNCE_LINES: usize = 20_000;
const DEBOUNCE_SECONDS: f64 = 0.3;

#[derive(Default)]
pub struct CompareState {
    pub options: DiffOptions,
    result: Comparison,
    /// Revisions, options and theme the result was computed for.
    key: Option<(u64, u64, DiffOptions, bool)>,
    /// Revisions of the last edit seen while waiting, and when it was seen.
    edited: Option<((u64, u64), f64)>,
    left: Decorations,
    right: Decorations,
    offsets: (Vec2, Vec2),
}

pub struct CompareView {
    pub left: Side,
    pub right: Side,
    pub state: CompareState,
}

impl CompareView {
    pub fn new(left: Side, right: Side) -> Self {
        Self {
            left,
            right,
            state: CompareState::default(),
        }
    }

    pub fn uses_tab(&self, id: u64) -> bool {
        [&self.left, &self.right]
            .into_iter()
            .any(|side| matches!(side, Side::Tab(tab) if *tab == id))
    }
}

impl CompareState {
    pub fn result(&self) -> &Comparison {
        &self.result
    }

    pub fn decorations(&self) -> (&Decorations, &Decorations) {
        (&self.left, &self.right)
    }

    /// Recomputes the comparison if either document, the options or the theme changed.
    /// For large documents, edits wait for a pause; returns the seconds until a retry then.
    pub fn update(
        &mut self,
        left: &Document,
        right: &Document,
        dark: bool,
        now: f64,
    ) -> Option<f64> {
        let key = (left.revision(), right.revision(), self.options, dark);
        if self.key == Some(key) {
            return None;
        }
        let only_edited = self
            .key
            .is_some_and(|(_, _, options, was_dark)| options == self.options && was_dark == dark);
        let large = left.text().len_lines().max(right.text().len_lines()) > DEBOUNCE_LINES;
        if only_edited && large {
            let revisions = (key.0, key.1);
            let edited_at = match self.edited {
                Some((seen, at)) if seen == revisions => at,
                _ => {
                    self.edited = Some((revisions, now));
                    now
                }
            };
            let waited = now - edited_at;
            if waited < DEBOUNCE_SECONDS {
                return Some(DEBOUNCE_SECONDS - waited);
            }
        }
        self.key = Some(key);
        self.edited = None;
        self.result = diff::compare(left, right, self.options);
        (self.left, self.right) = decorate(&self.result, dark);
        None
    }

    /// Mirrors scrolling of one side to the other. Returns whether a repaint is needed.
    pub fn sync_scroll(&mut self, left: &mut EditorState, right: &mut EditorState) -> bool {
        let (l, r) = (left.scroll_offset(), right.scroll_offset());
        if l != self.offsets.0 {
            right.scroll_to_offset(l);
            self.offsets = (l, l);
            true
        } else if r != self.offsets.1 {
            left.scroll_to_offset(r);
            self.offsets = (r, r);
            true
        } else {
            false
        }
    }

    /// Moves both cursors to the next (or previous) difference after the left cursor.
    pub fn go_to_difference(
        &self,
        forward: bool,
        (left_doc, left): (&Document, &mut EditorState),
        (right_doc, right): (&Document, &mut EditorState),
    ) {
        let hunks = &self.result.hunks;
        let (line, _) = left.line_col(left_doc);
        let index = if forward {
            hunks
                .iter()
                .position(|h| h.left.start > line)
                .or((!hunks.is_empty()).then_some(0))
        } else {
            hunks
                .iter()
                .rposition(|h| h.left.start < line)
                .or(hunks.len().checked_sub(1))
        };
        let Some(hunk) = index.map(|i| &hunks[i]) else {
            return;
        };
        let start = |doc: &Document, line: usize| {
            let rope = doc.text();
            rope.line_to_char(line.min(rope.len_lines() - 1))
        };
        left.set_cursor(start(left_doc, hunk.left.start));
        right.set_cursor(start(right_doc, hunk.right.start));
    }

    /// Number of changed, added and removed blocks.
    pub fn counts(&self) -> (usize, usize, usize) {
        let count = |kind| self.result.hunks.iter().filter(|h| h.kind == kind).count();
        (
            count(HunkKind::Changed),
            count(HunkKind::Added),
            count(HunkKind::Removed),
        )
    }
}

fn decorate(result: &Comparison, dark: bool) -> (Decorations, Decorations) {
    let alpha = if dark { 70 } else { 55 };
    let removed = Color32::from_rgba_unmultiplied(220, 50, 50, alpha);
    let added = Color32::from_rgba_unmultiplied(40, 170, 70, alpha);
    let changed = Color32::from_rgba_unmultiplied(230, 180, 0, alpha);
    let inline = Color32::from_rgba_unmultiplied(240, 120, 0, alpha * 2);

    let (mut left, mut right) = (Decorations::default(), Decorations::default());
    for hunk in &result.hunks {
        match hunk.kind {
            HunkKind::Removed => left.backgrounds.push((hunk.left.clone(), removed)),
            HunkKind::Added => right.backgrounds.push((hunk.right.clone(), added)),
            HunkKind::Changed => {
                left.backgrounds.push((hunk.left.clone(), changed));
                right.backgrounds.push((hunk.right.clone(), changed));
            }
        }
    }
    left.spans = result
        .left_changes
        .iter()
        .map(|r| (r.clone(), inline))
        .collect();
    right.spans = result
        .right_changes
        .iter()
        .map(|r| (r.clone(), inline))
        .collect();
    (left.spacers, right.spacers) = result.spacers();
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::new();
        doc.replace(0..0, text);
        doc
    }

    #[test]
    fn recomputes_only_on_change() {
        let (left, mut right) = (doc("a\nb"), doc("a\nc"));
        let mut state = CompareState::default();
        assert_eq!(state.update(&left, &right, false, 0.0), None);
        assert_eq!(state.counts(), (1, 0, 0));
        assert_eq!(state.decorations().0.spans.len(), 1);

        right.replace(2..3, "b");
        assert_eq!(state.update(&left, &right, false, 0.0), None);
        assert!(state.result().is_identical());
    }

    #[test]
    fn waits_for_typing_pause_in_large_documents() {
        let text = "x\n".repeat(DEBOUNCE_LINES + 1);
        let (left, mut right) = (doc(&text), doc(&text));
        let mut state = CompareState::default();
        assert_eq!(state.update(&left, &right, false, 0.0), None);

        right.replace(0..1, "y");
        assert!(state.update(&left, &right, false, 1.0).is_some());
        assert!(state.result().is_identical(), "not yet recomputed");
        assert!(state.update(&left, &right, false, 1.1).is_some());
        assert_eq!(
            state.update(&left, &right, false, 1.0 + DEBOUNCE_SECONDS),
            None
        );
        assert_eq!(state.counts(), (1, 0, 0));
    }

    #[test]
    fn navigates_differences_with_wrap_around() {
        let (left, right) = (doc("x\na\nx\nb\nx"), doc("x\nA\nx\nB\nx"));
        let mut state = CompareState::default();
        state.update(&left, &right, false, 0.0);
        let (mut l, mut r) = (EditorState::default(), EditorState::default());
        for expected in [1, 3, 1] {
            state.go_to_difference(true, (&left, &mut l), (&right, &mut r));
            assert_eq!(l.line_col(&left).0, expected);
            assert_eq!(r.line_col(&right).0, expected);
        }
        state.go_to_difference(false, (&left, &mut l), (&right, &mut r));
        assert_eq!(l.line_col(&left).0, 3);
    }

    #[test]
    fn syncs_scrolling_from_the_side_that_moved() {
        let mut state = CompareState::default();
        let (mut l, mut r) = (EditorState::default(), EditorState::default());
        assert!(!state.sync_scroll(&mut l, &mut r));
        r.scroll_to_offset(Vec2::new(0.0, 40.0));
        assert!(state.sync_scroll(&mut l, &mut r));
        assert_eq!(l.scroll_offset().y, 40.0);
        assert!(!state.sync_scroll(&mut l, &mut r));
    }
}
