use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::sync::Arc;

use cuneiedit_core::{Document, Rope, fold};
use egui::{Event, Rect, Vec2};

use crate::movement::{self, column_at_display, display_column, line_len};
use crate::rows::RowMap;

/// More selections than this (e.g. a column selection over a huge file) are cut off.
const MAX_SELECTIONS: usize = 100_000;

/// Cursor, selection and view state of one editor. Keep one per open document.
#[derive(Clone, Debug, Default)]
pub struct EditorState {
    pub(crate) cursor: usize,
    pub(crate) anchor: usize,
    /// Further selections as `(anchor, cursor)`, sorted and not overlapping the others.
    pub(crate) extra: Vec<(usize, usize)>,
    /// Line and display column where a rectangular selection started.
    pub(crate) column_start: Option<(usize, usize)>,
    /// Horizontal position kept while moving up and down through shorter lines.
    pub(crate) preferred_x: Option<f32>,
    /// Uncommitted IME composition text.
    pub(crate) preedit: String,
    pub(crate) scroll_to_cursor: bool,
    pub(crate) focus_requested: bool,
    pub(crate) max_columns: Option<usize>,
    pub(crate) scroll_offset: Vec2,
    pub(crate) pending_scroll: Option<Vec2>,
    bookmarks: BTreeSet<usize>,
    /// Revision and line count the bookmarks belong to.
    bookmarks_at: Option<(u64, usize)>,
    /// Folded blocks: header line and the number of hidden lines after it.
    folds: BTreeMap<usize, usize>,
    /// Revision and line count the folds belong to.
    folds_at: Option<(u64, usize)>,
    /// Line to scroll into the middle of the view the next time the editor is shown.
    pub(crate) scroll_to_line: Option<usize>,
    /// Lines shown in the last frame.
    pub(crate) visible_lines: Range<usize>,
    pub(crate) wrap_cache: Option<WrapCache>,
    pub(crate) segment_cache: SegmentCache,
    /// The row map of the last frame and what it was built from.
    pub(crate) row_cache: Option<(RowKey, Arc<RowMap>)>,
    /// Revision, cursor and matching bracket pair found for them.
    pub(crate) bracket_cache: Option<BracketCache>,
    pub(crate) tag_cache: Option<TagCache>,
    /// Input recorded for a macro, while recording.
    pub(crate) recording: Option<Vec<Event>>,
    /// Recorded input to replay the next time the editor is shown, and how often.
    pub(crate) playback: Option<(Vec<Event>, Repeat)>,
    /// Cursor position on screen as of the last frame.
    pub(crate) cursor_screen_rect: Option<Rect>,
}

/// How often a macro is replayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Times(usize),
    /// Until the cursor stops moving forward or reaches the end of the document.
    UntilEnd,
}

/// Revision, cursor and the matching bracket pair found for them.
pub(crate) type BracketCache = (u64, usize, Option<(usize, usize)>);

/// Revision, cursor and the matching tag names found for them.
pub(crate) type TagCache = (u64, usize, Option<(Range<usize>, Range<usize>)>);

/// Extra visual rows of wrapped lines, valid for one document revision and width.
#[derive(Clone, Debug)]
pub(crate) struct WrapCache {
    pub revision: u64,
    pub width: usize,
    pub tab_width: usize,
    /// Line count of the document.
    pub lines: usize,
    /// `(line, extra rows)` for every line that wraps.
    pub extra_rows: Vec<(usize, usize)>,
}

/// Wrap segments of long lines, keyed by `(line, length)`, for one revision, wrap width and
/// tab width.
#[derive(Clone, Debug, Default)]
pub(crate) struct SegmentCache {
    pub key: (u64, usize, usize),
    pub lines: HashMap<(usize, usize), Arc<[Range<usize>]>>,
}

/// Everything a row map depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RowKey {
    pub revision: u64,
    pub wrap_width: Option<usize>,
    pub tab_width: usize,
    pub spacers: u64,
    pub folds: (usize, usize, usize),
}

impl EditorState {
    pub fn start_recording(&mut self) {
        self.recording = Some(Vec::new());
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Stops recording and returns the recorded input.
    pub fn stop_recording(&mut self) -> Vec<Event> {
        self.recording.take().unwrap_or_default()
    }

    /// Replays recorded input the next time the editor is shown.
    pub fn play(&mut self, events: Vec<Event>, repeat: Repeat) {
        self.playback = Some((events, repeat));
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Where the cursor was drawn in the last frame, in screen coordinates, e.g. for popups.
    pub fn cursor_screen_rect(&self) -> Option<Rect> {
        self.cursor_screen_rect
    }

    /// Scroll position as of the last frame.
    pub fn scroll_offset(&self) -> Vec2 {
        self.scroll_offset
    }

    /// Scrolls to `offset` the next time the editor is shown.
    pub fn scroll_to_offset(&mut self, offset: Vec2) {
        self.pending_scroll = Some(offset);
        self.scroll_offset = offset;
    }

    /// Bookmarked lines; call [`Self::track_bookmarks`] first if the document changed.
    pub fn bookmarks(&self) -> &BTreeSet<usize> {
        &self.bookmarks
    }

    pub fn toggle_bookmark(&mut self, line: usize) {
        if !self.bookmarks.remove(&line) {
            self.bookmarks.insert(line);
        }
    }

    pub fn clear_bookmarks(&mut self) {
        self.bookmarks.clear();
    }

    /// Replaces all bookmarks with `lines` of the current `doc`.
    pub fn set_bookmarks(&mut self, doc: &Document, lines: impl IntoIterator<Item = usize>) {
        self.bookmarks = lines.into_iter().collect();
        self.bookmarks_at = Some((doc.revision(), doc.text().len_lines()));
    }

    /// Moves the cursor to the next (or previous) bookmark, wrapping around.
    pub fn go_to_bookmark(&mut self, doc: &Document, forward: bool) {
        let (line, _) = self.line_col(doc);
        let target = if forward {
            self.bookmarks
                .range(line + 1..)
                .next()
                .or(self.bookmarks.first())
        } else {
            self.bookmarks
                .range(..line)
                .next_back()
                .or(self.bookmarks.last())
        };
        if let Some(&target) = target {
            let rope = doc.text();
            self.set_cursor(rope.line_to_char(target.min(rope.len_lines() - 1)));
        }
    }

    /// Shifts bookmarks below an edit by the number of lines it added or removed.
    pub fn track_bookmarks(&mut self, doc: &Document) {
        let rope = doc.text();
        let now = (doc.revision(), rope.len_lines());
        let Some((revision, line_count)) = self.bookmarks_at.replace(now) else {
            return;
        };
        let delta = now.1 as isize - line_count as isize;
        if delta == 0 || self.bookmarks.is_empty() {
            return;
        }
        let Some(changed) = doc.first_change_since(revision) else {
            return;
        };
        let changed_line = rope.char_to_line(changed.min(rope.len_chars()));
        self.bookmarks = self
            .bookmarks
            .iter()
            .map(|&line| {
                if line <= changed_line {
                    line
                } else {
                    line.saturating_add_signed(delta).max(changed_line)
                }
            })
            .filter(|&line| line < now.1)
            .collect();
    }

    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }

    /// All selections including the main one, sorted by position.
    pub fn selections(&self) -> Vec<Range<usize>> {
        self.all_sorted()
            .0
            .into_iter()
            .map(|(a, c)| a.min(c)..a.max(c))
            .collect()
    }

    pub fn has_multiple_selections(&self) -> bool {
        !self.extra.is_empty()
    }

    /// Keeps only the main selection.
    pub fn clear_extra_selections(&mut self) {
        self.extra.clear();
    }

    /// Adds a cursor at `pos`, or removes the cursor there if there is one.
    pub fn add_cursor(&mut self, pos: usize) {
        let (mut all, primary) = self.all_sorted();
        if all.len() > 1
            && let Some(index) = all.iter().position(|&(a, c)| a == pos && c == pos)
        {
            all.remove(index);
            let primary = if index == primary {
                all.len() - 1
            } else {
                primary - usize::from(index < primary)
            };
            self.set_all(all, primary);
            return;
        }
        all.push((pos, pos));
        let last = all.len() - 1;
        self.set_all(all, last);
    }

    /// Adds a cursor in the line below the lowest (or above the highest) cursor, at the same
    /// display column.
    pub fn add_cursor_vertically(&mut self, doc: &Document, down: bool, tab_width: usize) {
        let (all, _) = self.all_sorted();
        let rope = doc.text();
        let edge = if down { all.last() } else { all.first() };
        let Some(&(_, cursor)) = edge else {
            return;
        };
        let (line, col) = movement::line_col(rope, cursor.min(rope.len_chars()));
        let target = if down {
            Some(line + 1).filter(|&l| l < rope.len_lines())
        } else {
            line.checked_sub(1)
        };
        let Some(target) = target else {
            return;
        };
        let display = display_column(rope.line(line), col, tab_width);
        let pos = position_at_display(rope, target, display, tab_width);
        let mut all = all;
        all.push((pos, pos));
        let last = all.len() - 1;
        self.set_all(all, last);
    }

    /// Selects the same display columns in every line from `from` to `to` (line, column).
    pub(crate) fn column_select(
        &mut self,
        doc: &Document,
        from: (usize, usize),
        to: (usize, usize),
        tab_width: usize,
    ) {
        let rope = doc.text();
        let last = rope.len_lines() - 1;
        let (top, bottom) = (from.0.min(to.0).min(last), from.0.max(to.0).min(last));
        let all: Vec<(usize, usize)> = (top..=bottom)
            .take(MAX_SELECTIONS)
            .map(|line| {
                (
                    position_at_display(rope, line, from.1, tab_width),
                    position_at_display(rope, line, to.1, tab_width),
                )
            })
            .collect();
        let primary = if to.0 >= from.0 { all.len() - 1 } else { 0 };
        self.set_all(all, primary);
    }

    /// Puts a cursor in every line from the cursor's line down, at the cursor's display
    /// column (a final empty line is left out). Returns, per cursor, how many columns its
    /// line is too short to reach that column.
    pub fn column_cursors(&mut self, doc: &Document, tab_width: usize) -> Vec<usize> {
        let rope = doc.text();
        let (line, col) = self.line_col(doc);
        let display = display_column(rope.line(line), col, tab_width);
        let mut last = rope.len_lines();
        if last > line + 1 && line_len(rope, last - 1) == 0 {
            last -= 1;
        }
        let mut missing = Vec::new();
        let all: Vec<(usize, usize)> = (line..last)
            .take(MAX_SELECTIONS)
            .map(|l| {
                let (col, reached) = column_at_display(rope.line(l), display, tab_width);
                missing.push(display.saturating_sub(reached));
                let pos = rope.line_to_char(l) + col.min(line_len(rope, l));
                (pos, pos)
            })
            .collect();
        self.set_all(all, 0);
        missing
    }

    /// All selections as `(anchor, cursor)`, sorted, and the index of the main one.
    pub(crate) fn all_sorted(&self) -> (Vec<(usize, usize)>, usize) {
        let start = |s: &(usize, usize)| s.0.min(s.1);
        let main = (self.anchor, self.cursor);
        let mut all = self.extra.clone();
        let index = all.partition_point(|s| start(s) < start(&main));
        all.insert(index, main);
        (all, index)
    }

    /// Replaces all selections; overlapping ones are merged. `primary` is the main one.
    pub(crate) fn set_all(&mut self, all: Vec<(usize, usize)>, primary: usize) {
        if primary >= all.len() {
            return;
        }
        let mut sorted: Vec<(usize, (usize, usize))> = all.into_iter().enumerate().collect();
        sorted.sort_by_key(|&(_, (a, c))| (a.min(c), a.max(c)));
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(sorted.len());
        let mut main_index = 0;
        for (index, (a, c)) in sorted {
            let (start, end) = (a.min(c), a.max(c));
            if let Some(last) = merged.last_mut() {
                let (last_start, last_end) = (last.0.min(last.1), last.0.max(last.1));
                let touching_cursors =
                    start == last_end && (start == end || last_start == last_end);
                if start < last_end || touching_cursors {
                    let end = end.max(last_end);
                    *last = if last.1 >= last.0 {
                        (last_start, end)
                    } else {
                        (end, last_start)
                    };
                    if index == primary {
                        main_index = merged.len() - 1;
                    }
                    continue;
                }
            }
            if index == primary {
                main_index = merged.len();
            }
            merged.push((a, c));
        }
        merged.truncate(MAX_SELECTIONS);
        let main_index = main_index.min(merged.len() - 1);
        (self.anchor, self.cursor) = merged.remove(main_index);
        self.extra = merged;
        self.preferred_x = None;
        self.scroll_to_cursor = true;
    }

    /// Replaces every selection with the text `edit` returns for it (it may also widen the
    /// range, e.g. to delete a char), as one undo step. Cursors end up after the new text.
    pub(crate) fn replace_each(
        &mut self,
        doc: &mut Document,
        mut edit: impl FnMut(&Rope, Range<usize>, usize) -> (Range<usize>, String),
    ) {
        let (all, primary) = self.all_sorted();
        let rope = doc.text();
        let edits: Vec<(Range<usize>, String)> = all
            .iter()
            .map(|&(a, c)| edit(rope, a.min(c)..a.max(c), c))
            .collect();
        let ends = doc.replace_many(&edits);
        self.set_all(ends.into_iter().map(|e| (e, e)).collect(), primary);
    }

    /// Folded blocks as `(header line, hidden lines)`; call [`Self::track_folds`] first if the
    /// document changed.
    pub fn folds(&self) -> &BTreeMap<usize, usize> {
        &self.folds
    }

    /// Folds or unfolds the block starting at `line`. Returns whether anything changed.
    pub fn toggle_fold(&mut self, doc: &Document, line: usize, tab_width: usize) -> bool {
        if self.folds.remove(&line).is_some() {
            return true;
        }
        match fold::fold_range(doc.text(), line, tab_width) {
            Some(range) => {
                self.fold(doc, line, range.len());
                true
            }
            None => false,
        }
    }

    /// Folds the innermost block around the cursor, or unfolds it if `fold` is false.
    pub fn fold_current(&mut self, doc: &Document, tab_width: usize, fold: bool) {
        let (line, _) = self.line_col(doc);
        if !fold {
            let around = self
                .folds
                .range(..=line)
                .next_back()
                .filter(|&(&h, &n)| line <= h + n)
                .map(|(&h, _)| h);
            if let Some(header) = around {
                self.folds.remove(&header);
            }
            return;
        }
        if let Some((header, count)) = fold::enclosing_header(doc.text(), line, tab_width) {
            self.fold(doc, header, count);
        }
    }

    /// Folds every outermost block.
    pub fn fold_all(&mut self, doc: &Document, tab_width: usize) {
        for (header, count) in fold::top_level_folds(doc.text(), tab_width) {
            self.fold(doc, header, count);
        }
    }

    pub fn unfold_all(&mut self) {
        self.folds.clear();
    }

    /// Hides the selected lines; the first line of the document always stays visible.
    pub fn hide_lines(&mut self, doc: &Document) {
        let rope = doc.text();
        let selection = self.selection();
        let first = rope.char_to_line(selection.start).max(1);
        let mut last = rope.char_to_line(selection.end);
        if last > first && selection.end == rope.line_to_char(last) && !selection.is_empty() {
            last -= 1;
        }
        if first <= last && last < rope.len_lines() {
            self.fold(doc, first - 1, last - first + 1);
        }
    }

    fn fold(&mut self, doc: &Document, header: usize, count: usize) {
        if count == 0 {
            return;
        }
        self.track_folds(doc);
        self.folds.insert(header, count);
        let rope = doc.text();
        let (line, _) = self.line_col(doc);
        if line > header && line <= header + count {
            self.set_cursor(movement::line_end(rope, header));
        }
    }

    /// Unfolds blocks that hide `line`.
    pub(crate) fn reveal(&mut self, line: usize) {
        while let Some((&header, _)) = self
            .folds
            .range(..line)
            .rev()
            .find(|&(&h, &n)| line <= h + n)
        {
            self.folds.remove(&header);
        }
    }

    /// Moves folds below an edit by the number of lines it added or removed.
    pub fn track_folds(&mut self, doc: &Document) {
        let rope = doc.text();
        let now = (doc.revision(), rope.len_lines());
        let Some((revision, line_count)) = self.folds_at.replace(now) else {
            return;
        };
        let delta = now.1 as isize - line_count as isize;
        if delta == 0 || self.folds.is_empty() {
            return;
        }
        let Some(changed) = doc.first_change_since(revision) else {
            return;
        };
        let changed = changed.min(rope.len_chars());
        let changed_line = rope.char_to_line(changed);
        // Lines inserted at the very start of a header push the whole fold down.
        let at_line_start = changed == rope.line_to_char(changed_line);
        self.folds = self
            .folds
            .iter()
            .filter_map(|(&header, &count)| {
                if header + count < changed_line {
                    Some((header, count))
                } else if header < changed_line || (header == changed_line && !at_line_start) {
                    let count = count.saturating_add_signed(delta);
                    (count > 0).then_some((header, count))
                } else {
                    let moved = header as isize + delta;
                    (moved >= changed_line as isize).then_some((moved as usize, count))
                }
            })
            .filter(|&(header, count)| header + count < now.1)
            .collect();
    }

    /// Scrolls `line` into the middle of the view the next time the editor is shown.
    pub fn scroll_to_line(&mut self, line: usize) {
        self.scroll_to_line = Some(line);
    }

    /// Lines shown in the last frame.
    pub fn visible_lines(&self) -> Range<usize> {
        self.visible_lines.clone()
    }

    /// Zero-based line and column of the cursor.
    pub fn line_col(&self, doc: &Document) -> (usize, usize) {
        movement::line_col(doc.text(), self.cursor.min(doc.text().len_chars()))
    }

    pub fn selected_text(&self, doc: &Document) -> String {
        doc.slice(self.selection())
    }

    /// Selects from `anchor` to `cursor` and scrolls the cursor into view. Other selections
    /// are removed.
    pub fn set_selection(&mut self, anchor: usize, cursor: usize) {
        self.anchor = anchor;
        self.cursor = cursor;
        self.extra.clear();
        self.preferred_x = None;
        self.scroll_to_cursor = true;
    }

    pub fn set_cursor(&mut self, pos: usize) {
        self.set_selection(pos, pos);
    }

    pub fn select_all(&mut self, doc: &Document) {
        self.anchor = 0;
        self.cursor = doc.text().len_chars();
        self.extra.clear();
        self.preferred_x = None;
    }

    /// Gives the editor keyboard focus the next time it is shown.
    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    /// Replaces the selection (every selection, if there are several) with `text`.
    pub fn insert(&mut self, doc: &mut Document, text: &str) {
        if self.extra.is_empty() {
            let end = doc.replace(self.selection(), text);
            self.set_cursor(end);
        } else {
            self.replace_each(doc, |_, range, _| (range, text.to_owned()));
        }
    }

    /// Replaces every selection with its own text, e.g. numbers counting up.
    pub fn insert_each(&mut self, doc: &mut Document, mut text: impl FnMut(usize) -> String) {
        let mut index = 0;
        self.replace_each(doc, |_, range, _| {
            index += 1;
            (range, text(index - 1))
        });
    }

    pub fn undo(&mut self, doc: &mut Document) {
        if let Some(pos) = doc.undo() {
            self.set_cursor(pos);
        }
    }

    pub fn redo(&mut self, doc: &mut Document) {
        if let Some(pos) = doc.redo() {
            self.set_cursor(pos);
        }
    }

    /// Copies the selected text; several selections are copied as lines.
    pub fn copy(&self, doc: &Document, ctx: &egui::Context) {
        let selections = self.selections();
        if selections.iter().any(|s| !s.is_empty()) {
            let texts: Vec<String> = selections.into_iter().map(|s| doc.slice(s)).collect();
            ctx.copy_text(texts.join("\n"));
        }
    }

    pub fn cut(&mut self, doc: &mut Document, ctx: &egui::Context) {
        if self.selections().iter().any(|s| !s.is_empty()) {
            self.copy(doc, ctx);
            doc.seal_undo_group();
            self.insert(doc, "");
            doc.seal_undo_group();
        }
    }

    pub(crate) fn clamp(&mut self, doc: &Document) {
        let len = doc.text().len_chars();
        self.cursor = self.cursor.min(len);
        self.anchor = self.anchor.min(len);
        if self.extra.iter().any(|&(a, c)| a.max(c) > len) {
            self.extra.clear();
        }
    }
}

/// Char position in `line` at display column `display`, or the line end if it is shorter.
fn position_at_display(rope: &Rope, line: usize, display: usize, tab_width: usize) -> usize {
    let (col, _) = column_at_display(rope.line(line), display, tab_width);
    rope.line_to_char(line) + col.min(line_len(rope, line))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_replaces_selection() {
        let mut doc = Document::new();
        let mut state = EditorState::default();
        state.insert(&mut doc, "hello world");
        state.set_selection(6, 11);
        assert_eq!(state.selected_text(&doc), "world");
        state.insert(&mut doc, "there");
        assert_eq!(doc.text().to_string(), "hello there");
        assert_eq!(state.selection(), 11..11);
        assert_eq!(state.line_col(&doc), (0, 11));
    }

    #[test]
    fn bookmarks_follow_inserted_and_removed_lines() {
        let mut doc = Document::new();
        let mut state = EditorState::default();
        doc.replace(0..0, "a\nb\nc\nd\n");
        state.track_bookmarks(&doc);
        state.toggle_bookmark(0);
        state.toggle_bookmark(2);

        doc.replace(2..2, "new\n");
        state.track_bookmarks(&doc);
        assert_eq!(
            state.bookmarks().iter().copied().collect::<Vec<_>>(),
            vec![0, 3]
        );

        doc.replace(0..6, "");
        state.track_bookmarks(&doc);
        assert_eq!(
            state.bookmarks().iter().copied().collect::<Vec<_>>(),
            vec![0, 1]
        );

        state.set_cursor(0);
        state.go_to_bookmark(&doc, true);
        assert_eq!(state.line_col(&doc), (1, 0));
        state.go_to_bookmark(&doc, true);
        assert_eq!(state.line_col(&doc), (0, 0));
        state.go_to_bookmark(&doc, false);
        assert_eq!(state.line_col(&doc), (1, 0));
    }

    #[test]
    fn edits_at_every_cursor() {
        let mut doc = Document::new();
        doc.replace(0..0, "ab\ncd\nef");
        let mut state = EditorState::default();
        state.set_cursor(1);
        state.add_cursor_vertically(&doc, true, 4);
        state.add_cursor_vertically(&doc, true, 4);
        assert_eq!(state.selections(), vec![1..1, 4..4, 7..7]);
        state.insert(&mut doc, "X");
        assert_eq!(doc.text().to_string(), "aXb\ncXd\neXf");
        assert_eq!(state.selections(), vec![2..2, 6..6, 10..10]);
        state.undo(&mut doc);
        assert_eq!(doc.text().to_string(), "ab\ncd\nef");
        assert!(!state.has_multiple_selections());
    }

    #[test]
    fn selects_columns_and_merges_cursors() {
        let mut doc = Document::new();
        doc.replace(0..0, "abcd\nab\nabcd");
        let mut state = EditorState::default();
        state.column_select(&doc, (0, 1), (2, 3), 4);
        assert_eq!(state.selections(), vec![1..3, 6..7, 9..11]);
        assert_eq!(state.cursor(), 11);
        state.add_cursor(2);
        state.add_cursor(6);
        assert_eq!(
            state.selections().len(),
            3,
            "cursors inside selections merge"
        );
        let mut n = 0;
        state.insert_each(&mut doc, |i| {
            n += 1;
            i.to_string()
        });
        assert_eq!(doc.text().to_string(), "a0d\na1\na2d");
        assert_eq!(n, 3);
        assert_eq!(state.selections(), vec![2..2, 6..6, 9..9]);
        state.add_cursor(6);
        assert_eq!(
            state.selections().len(),
            2,
            "a second click removes a cursor"
        );
    }

    #[test]
    fn folds_hide_blocks_and_follow_edits() {
        let mut doc = Document::new();
        doc.replace(0..0, "fn a() {\n  x\n}\nfn b() {\n  y\n}\n");
        let mut state = EditorState::default();
        state.track_folds(&doc);
        state.set_cursor(11);
        assert!(state.toggle_fold(&doc, 0, 4));
        assert_eq!(state.cursor(), 8, "the cursor leaves the folded lines");
        assert!(state.toggle_fold(&doc, 3, 4));
        assert!(!state.toggle_fold(&doc, 1, 4));
        doc.replace(0..0, "// c\n");
        state.track_folds(&doc);
        assert_eq!(
            state
                .folds()
                .iter()
                .map(|(&h, &n)| (h, n))
                .collect::<Vec<_>>(),
            vec![(1, 1), (4, 1)]
        );
        state.reveal(5);
        assert_eq!(state.folds().len(), 1);
        state.unfold_all();
        state.fold_all(&doc, 4);
        assert_eq!(state.folds().len(), 2);
        state.set_cursor(doc.text().line_to_char(5));
        state.fold_current(&doc, 4, false);
        assert_eq!(state.folds().keys().copied().collect::<Vec<_>>(), vec![1]);
        state.set_selection(doc.text().line_to_char(5), doc.text().line_to_char(7));
        state.hide_lines(&doc);
        assert_eq!(state.folds().get(&4), Some(&2));
    }

    #[test]
    fn clamps_after_external_change() {
        let mut doc = Document::new();
        let mut state = EditorState::default();
        state.insert(&mut doc, "abc");
        doc.undo();
        state.clamp(&doc);
        assert_eq!(state.selection(), 0..0);
    }
}
