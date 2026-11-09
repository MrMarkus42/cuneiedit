use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use cuneiedit_core::highlight::{HUGE_LINE, Highlighter, Token, TokenKind};
use cuneiedit_core::{Document, Rope, fold, ops, transform};
use egui::os::OperatingSystem;
use egui::output::IMEOutput;
use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{
    Align, Align2, AsIdSalt, Color32, CursorIcon, Event, EventFilter, FontId, Galley, IMEPurpose,
    IdSalt, ImeEvent, Key, Modifiers, Pos2, Rect, Response, ScrollArea, Sense, Shape, Stroke, Ui,
    Visuals, pos2, vec2,
};

use crate::EditorState;
use crate::movement::{
    char_columns, column_at_display, display_column, expand_tabs, leading_whitespace, line_col,
    line_end, line_len, next_word, prev_word, smart_home, word_at, wrap_points,
};
use crate::rows::{RowKind, RowMap};
use crate::state::{Repeat, RowKey, SegmentCache, WrapCache};

const GUTTER_PADDING: f32 = 8.0;
/// Space in the gutter for bookmark markers.
const MARKER_WIDTH: f32 = 12.0;
/// Space in the gutter for fold markers.
const FOLD_WIDTH: f32 = 12.0;
const TEXT_PADDING: f32 = 4.0;
const CURSOR_WIDTH: f32 = 2.0;
/// Lines longer than this (in chars) are only laid out around the visible columns.
const LONG_LINE: usize = 2_000;
const LONG_LINE_MARGIN: usize = 256;
/// Lines the highlighter may lex per frame to catch up, e.g. after jumping to the end
/// (~15 ms on Apple Silicon).
const HIGHLIGHT_BUDGET: usize = 50_000;
/// Upper bound for macro replays "until the end", so a macro that edits forever stops.
const MAX_REPLAYS: usize = 100_000;
/// Longest selection whose other occurrences are highlighted.
const MAX_SMART_WORD: usize = 200;

/// Colors of the text area. Without one, the editor follows the egui visuals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub background: Color32,
    pub text: Color32,
    pub gutter: Color32,
    pub line_number: Color32,
    pub current_line: Color32,
    pub selection: Color32,
    pub cursor: Color32,
    pub keyword: Color32,
    pub ty: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub meta: Color32,
    pub function: Color32,
    pub escape: Color32,
}

impl Palette {
    pub fn from_visuals(visuals: &Visuals) -> Self {
        let rgb = Color32::from_rgb;
        let (keyword, ty, string, number, comment, meta, function, escape) = if visuals.dark_mode {
            (
                rgb(0xC5, 0x86, 0xC0),
                rgb(0x4E, 0xC9, 0xB0),
                rgb(0xCE, 0x91, 0x78),
                rgb(0xB5, 0xCE, 0xA8),
                rgb(0x6A, 0x99, 0x55),
                rgb(0x56, 0x9C, 0xD6),
                rgb(0xDC, 0xDC, 0xAA),
                rgb(0xD7, 0xBA, 0x7D),
            )
        } else {
            (
                rgb(0xAF, 0x00, 0xDB),
                rgb(0x26, 0x7F, 0x99),
                rgb(0xA3, 0x15, 0x15),
                rgb(0x09, 0x86, 0x58),
                rgb(0x00, 0x80, 0x00),
                rgb(0x00, 0x00, 0xFF),
                rgb(0x79, 0x5E, 0x26),
                rgb(0xEE, 0x00, 0x00),
            )
        };
        Self {
            background: visuals.extreme_bg_color,
            text: visuals.text_color(),
            gutter: visuals.panel_fill,
            line_number: visuals.weak_text_color(),
            current_line: visuals.faint_bg_color,
            selection: visuals.selection.bg_fill,
            cursor: visuals.text_cursor.stroke.color,
            keyword,
            ty,
            string,
            number,
            comment,
            meta,
            function,
            escape,
        }
    }

    /// Color of a token kind; `None` is plain text.
    pub fn color(&self, kind: Option<TokenKind>) -> Color32 {
        match kind {
            None => self.text,
            Some(TokenKind::Keyword) => self.keyword,
            Some(TokenKind::Type) => self.ty,
            Some(TokenKind::String) => self.string,
            Some(TokenKind::Number) => self.number,
            Some(TokenKind::Comment) => self.comment,
            Some(TokenKind::Meta) => self.meta,
            Some(TokenKind::Function) => self.function,
            Some(TokenKind::Escape) => self.escape,
        }
    }

    fn faint(&self) -> Color32 {
        self.line_number.gamma_multiply(0.6)
    }
}

/// Text editor widget for a [`Document`]. Only the visible lines are laid out and painted.
pub struct Editor<'a> {
    id_salt: IdSalt,
    doc: &'a mut Document,
    state: &'a mut EditorState,
    font: FontId,
    tab_width: usize,
    indent_with_spaces: bool,
    show_whitespace: bool,
    show_line_endings: bool,
    show_wrap_symbol: bool,
    indent_guides: bool,
    word_wrap: bool,
    auto_close: bool,
    edge_column: Option<usize>,
    read_only: bool,
    folding: bool,
    smart_highlight: bool,
    match_tags: bool,
    palette: Option<Palette>,
    highlights: &'a [Range<usize>],
    spans: &'a [(Range<usize>, Color32)],
    line_backgrounds: &'a [(Range<usize>, Color32)],
    gutter_marks: &'a [(Range<usize>, Color32)],
    spacers: &'a [(usize, usize)],
    highlighter: Option<&'a mut Highlighter>,
}

impl<'a> Editor<'a> {
    pub fn new(id_salt: impl AsIdSalt, doc: &'a mut Document, state: &'a mut EditorState) -> Self {
        Self {
            id_salt: IdSalt::new(id_salt),
            doc,
            state,
            font: FontId::monospace(14.0),
            tab_width: 4,
            indent_with_spaces: false,
            show_whitespace: false,
            show_line_endings: false,
            show_wrap_symbol: false,
            indent_guides: false,
            word_wrap: false,
            auto_close: false,
            edge_column: None,
            read_only: false,
            folding: false,
            smart_highlight: false,
            match_tags: false,
            palette: None,
            highlights: &[],
            spans: &[],
            line_backgrounds: &[],
            gutter_marks: &[],
            spacers: &[],
            highlighter: None,
        }
    }

    pub fn font(mut self, font: FontId) -> Self {
        self.font = font;
        self
    }

    pub fn tab_width(mut self, tab_width: usize) -> Self {
        self.tab_width = tab_width.max(1);
        self
    }

    pub fn indent_with_spaces(mut self, spaces: bool) -> Self {
        self.indent_with_spaces = spaces;
        self
    }

    /// Shows spaces as dots and tabs as arrows.
    pub fn show_whitespace(mut self, show: bool) -> Self {
        self.show_whitespace = show;
        self
    }

    /// Shows a ¶ at the end of every line.
    pub fn show_line_endings(mut self, show: bool) -> Self {
        self.show_line_endings = show;
        self
    }

    /// Marks where wrapped lines continue.
    pub fn show_wrap_symbol(mut self, show: bool) -> Self {
        self.show_wrap_symbol = show;
        self
    }

    /// Thin vertical lines at every indentation level.
    pub fn indent_guides(mut self, show: bool) -> Self {
        self.indent_guides = show;
        self
    }

    /// Wraps long lines at word boundaries instead of scrolling horizontally.
    pub fn word_wrap(mut self, wrap: bool) -> Self {
        self.word_wrap = wrap;
        self
    }

    /// Typing an opening bracket or quote also inserts the closing one.
    pub fn auto_close(mut self, auto_close: bool) -> Self {
        self.auto_close = auto_close;
        self
    }

    /// A vertical line after this many columns.
    pub fn edge_column(mut self, column: Option<usize>) -> Self {
        self.edge_column = column;
        self
    }

    /// Char ranges to mark, e.g. search matches. Must be sorted and non-overlapping.
    pub fn highlights(mut self, highlights: &'a [Range<usize>]) -> Self {
        self.highlights = highlights;
        self
    }

    /// Colored char ranges, e.g. changed characters. Must be sorted by start.
    pub fn spans(mut self, spans: &'a [(Range<usize>, Color32)]) -> Self {
        self.spans = spans;
        self
    }

    /// Background colors for line ranges. Must be sorted by start.
    pub fn line_backgrounds(mut self, backgrounds: &'a [(Range<usize>, Color32)]) -> Self {
        self.line_backgrounds = backgrounds;
        self
    }

    /// Colored bars in the gutter for line ranges, e.g. changed lines. Sorted by start.
    pub fn gutter_marks(mut self, marks: &'a [(Range<usize>, Color32)]) -> Self {
        self.gutter_marks = marks;
        self
    }

    /// Blank rows shown above lines, as `(line, count)`, e.g. to align two documents.
    pub fn spacers(mut self, spacers: &'a [(usize, usize)]) -> Self {
        self.spacers = spacers;
        self
    }

    /// Allows moving the cursor, selecting and copying, but no changes.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Fold markers in the gutter; a click folds or unfolds the block.
    pub fn folding(mut self, folding: bool) -> Self {
        self.folding = folding;
        self
    }

    /// Highlights other occurrences of a selected word.
    pub fn smart_highlight(mut self, on: bool) -> Self {
        self.smart_highlight = on;
        self
    }

    /// Highlights the markup tag at the cursor and its partner.
    pub fn match_tags(mut self, on: bool) -> Self {
        self.match_tags = on;
        self
    }

    /// Colors of the text area instead of those of the egui visuals.
    pub fn palette(mut self, palette: Option<Palette>) -> Self {
        self.palette = palette;
        self
    }

    /// Syntax highlighting; the highlighter must belong to this document.
    pub fn highlighter(mut self, highlighter: &'a mut Highlighter) -> Self {
        self.highlighter = Some(highlighter);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let Self {
            id_salt,
            doc,
            state,
            font,
            tab_width,
            indent_with_spaces,
            show_whitespace,
            show_line_endings,
            show_wrap_symbol,
            indent_guides,
            word_wrap,
            auto_close,
            edge_column,
            read_only,
            folding,
            smart_highlight,
            match_tags,
            palette,
            highlights,
            spans,
            line_backgrounds,
            gutter_marks,
            spacers,
            mut highlighter,
        } = self;
        state.clamp(doc);
        state.track_bookmarks(doc);
        state.track_folds(doc);
        let palette = palette.unwrap_or_else(|| Palette::from_visuals(ui.visuals()));

        let id = ui.make_persistent_id(id_salt);
        let (row_height, char_width) = ui
            .ctx()
            .fonts_mut(|f| (f.row_height(&font), f.glyph_width(&font, '0')));
        let mac = ui.ctx().os() == OperatingSystem::Mac;
        let line_count = doc.text().len_lines();
        let digits = line_count.to_string().len().max(3);
        let fold_width = if folding { FOLD_WIDTH } else { 0.0 };
        let gutter_width =
            digits as f32 * char_width + 2.0 * GUTTER_PADDING + MARKER_WIDTH + fold_width;
        let available_width = ui.available_width();
        let wrap_width = word_wrap.then(|| {
            let text_width = available_width - gutter_width - 3.0 * TEXT_PADDING - 12.0;
            ((text_width / char_width) as usize).max(10)
        });

        let mut scroll_area = ScrollArea::both()
            .id_salt(id.with("scroll"))
            .auto_shrink(false);
        if let Some(offset) = state.pending_scroll.take() {
            scroll_area = scroll_area.scroll_offset(offset);
        }
        let output = scroll_area.show_viewport(ui, |ui, viewport| {
            let first_visible_column =
                ((viewport.min.x - gutter_width - TEXT_PADDING).max(0.0) / char_width) as usize;
            let layouter = Layouter {
                ctx: ui.ctx().clone(),
                font: font.clone(),
                palette,
                tab_width,
                char_width,
                wrap_width,
                visible_columns: first_visible_column
                    ..first_visible_column + (viewport.width() / char_width) as usize + 1,
                long_segments: RefCell::new(std::mem::take(&mut state.segment_cache)),
                segment_revision: Cell::new(doc.revision()),
            };
            let origin = ui.max_rect().min;
            let page_rows = ((viewport.height() / row_height) as usize).max(1);

            // Input first, so the frame shows its result.
            let rows = layouter.row_map(doc, state, spacers);
            let content_rect = content_rect(
                origin,
                viewport,
                &rows,
                doc,
                state,
                gutter_width,
                char_width,
                row_height,
                wrap_width.is_some(),
            );
            let response = ui
                .interact(content_rect, id, Sense::click_and_drag())
                .on_hover_cursor(CursorIcon::Text);
            ui.advance_cursor_after_rect(content_rect);

            let geometry = Geometry {
                origin,
                text_left: origin.x + gutter_width + TEXT_PADDING,
                gutter_right: origin.x + viewport.min.x + gutter_width,
                fold_width,
                row_height,
                rows: &rows,
            };
            let mut input = Input {
                doc,
                state,
                layouter: &layouter,
                rows: &rows,
                mac,
                page_rows,
                indent_with_spaces,
                auto_close,
                read_only,
                folding,
            };
            if std::mem::take(&mut input.state.focus_requested) {
                response.request_focus();
            }
            let folds_before = fold_fingerprint(input.state);
            input.handle_pointer(ui, &response, &geometry);
            let revision = input.doc.revision();
            input.replay(ui.ctx());
            let has_focus = response.has_focus();
            if has_focus {
                let filter = EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    // With several cursors, Escape goes back to one.
                    escape: input.state.has_multiple_selections(),
                };
                ui.memory_mut(|mem| mem.set_focus_lock_filter(id, filter));
                for event in ui.input(|i| i.filtered_events(&filter)) {
                    if let Some(recording) = &mut input.state.recording
                        && is_recordable(&event)
                    {
                        recording.push(event.clone());
                    }
                    input.handle_event(ui.ctx(), &event);
                }
            }
            let changed = input.doc.revision() != revision;
            let (doc, state) = (input.doc, input.state);
            if changed && wrap_width.is_none() {
                // Grow only; a full rescan per keystroke would be O(file size).
                let rope = doc.text();
                let edited = line_len(rope, rope.char_to_line(state.cursor));
                state.max_columns = Some(state.max_columns.unwrap_or(0).max(edited));
            }
            if changed {
                state.track_folds(doc);
            }
            if !state.folds().is_empty() {
                state.reveal(doc.text().char_to_line(state.cursor));
            }
            if let Some(line) = state.scroll_to_line {
                state.reveal(line.min(doc.text().len_lines() - 1));
            }
            // Edits may have changed how lines wrap, clicks what is folded.
            let rebuilt;
            let rows =
                if (changed && wrap_width.is_some()) || fold_fingerprint(state) != folds_before {
                    rebuilt = layouter.row_map(doc, state, spacers);
                    &rebuilt
                } else {
                    &rows
                };
            let geometry = Geometry { rows, ..geometry };
            if let Some(line) = state.scroll_to_line.take() {
                let top = geometry.row_top(rows.row(line));
                let target = Rect::from_x_y_ranges(
                    origin.x + viewport.min.x..=origin.x + viewport.min.x + 1.0,
                    top..=top + row_height,
                );
                ui.scroll_to_rect(target, Some(Align::Center));
            }

            if let Some(highlighter) = highlighter.as_deref_mut() {
                let (last_visible, _) = rows.line_at((viewport.max.y / row_height) as usize);
                if !highlighter.prepare(doc, last_visible, HIGHLIGHT_BUDGET) {
                    ui.ctx().request_repaint();
                }
            }

            let brackets = matching_brackets(doc, state);
            let tags = if match_tags {
                matching_tags(doc, state)
            } else {
                None
            };
            let smart_word = if smart_highlight {
                smart_word(doc, state)
            } else {
                None
            };
            let view = View {
                doc,
                state,
                layouter: &layouter,
                geometry,
                viewport,
                gutter_width,
                has_focus,
                highlights,
                spans,
                line_backgrounds,
                gutter_marks,
                highlighter: highlighter.as_deref(),
                brackets,
                tags,
                smart_word,
                folding,
                show_whitespace,
                show_line_endings,
                show_wrap_symbol,
                indent_guides,
                edge_column,
            };
            let cursor_rect = view.pos_rect(view.state.cursor);
            let (widest_visible, visible_lines) = view.paint(ui, cursor_rect);
            view.state.visible_lines = visible_lines;
            if wrap_width.is_none() && widest_visible > view.state.max_columns.unwrap_or(0) {
                view.state.max_columns = Some(widest_visible);
            }

            let to_global = ui
                .ctx()
                .layer_transform_to_global(ui.layer_id())
                .unwrap_or_default();
            view.state.cursor_screen_rect = Some(to_global * cursor_rect);
            if has_focus {
                let visible = viewport.translate(origin.to_vec2());
                ui.output_mut(|o| {
                    o.ime = Some(IMEOutput {
                        purpose: IMEPurpose::Normal,
                        rect: to_global * visible,
                        cursor_rect: to_global * cursor_rect,
                        should_interrupt_composition: false,
                    });
                });
            }
            if std::mem::take(&mut view.state.scroll_to_cursor) {
                let mut target = cursor_rect.expand(row_height);
                target.min.x -= gutter_width + TEXT_PADDING;
                ui.scroll_to_rect(target, None);
            }

            let mut response = response;
            if changed {
                response.mark_changed();
            }
            view.state.segment_cache = view.layouter.long_segments.take();
            response
        });
        state.scroll_offset = output.state.clamped_offset();
        output.inner
    }
}

#[expect(clippy::too_many_arguments)]
fn content_rect(
    origin: Pos2,
    viewport: Rect,
    rows: &RowMap,
    doc: &Document,
    state: &mut EditorState,
    gutter_width: f32,
    char_width: f32,
    row_height: f32,
    wrapped: bool,
) -> Rect {
    let width = if wrapped {
        viewport.width()
    } else {
        let max_columns = *state
            .max_columns
            .get_or_insert_with(|| max_line_chars(doc.text()));
        gutter_width + TEXT_PADDING + (max_columns + 2) as f32 * char_width
    };
    let height = rows.total_rows(doc.text().len_lines()) as f32 * row_height
        // Allow scrolling the last line up to the top.
        + (viewport.height() - row_height).max(0.0);
    Rect::from_min_size(origin, vec2(width, height).max(viewport.size()))
}

/// Longest line in chars; tabs are widened while painting.
fn max_line_chars(rope: &Rope) -> usize {
    (0..rope.len_lines())
        .map(|line| line_len(rope, line))
        .max()
        .unwrap_or(0)
}

fn matching_brackets(doc: &Document, state: &mut EditorState) -> Option<Brackets> {
    let rope = doc.text();
    let cursor = state.cursor;
    let at_bracket = [cursor, cursor.wrapping_sub(1)].into_iter().find(|&p| {
        p < rope.len_chars() && matches!(rope.char(p), '(' | ')' | '[' | ']' | '{' | '}')
    })?;
    let key = (doc.revision(), cursor);
    let pair = match state.bracket_cache {
        Some((revision, pos, pair)) if (revision, pos) == key => pair,
        _ => {
            let pair = transform::matching_bracket(rope, cursor);
            state.bracket_cache = Some((key.0, key.1, pair));
            pair
        }
    };
    Some(match pair {
        Some((a, b)) => Brackets::Matched(a, b),
        None => Brackets::Unmatched(at_bracket),
    })
}

#[derive(Clone, Copy)]
enum Brackets {
    Matched(usize, usize),
    Unmatched(usize),
}

/// Changes whenever folds are added or removed.
fn fold_fingerprint(state: &EditorState) -> (usize, usize, usize) {
    state
        .folds()
        .iter()
        .fold((0, 0, 0), |(n, h, c), (&header, &count)| {
            (n + 1, h ^ header.rotate_left(7) ^ n, c.wrapping_add(count))
        })
}

fn matching_tags(doc: &Document, state: &mut EditorState) -> Option<(Range<usize>, Range<usize>)> {
    let key = (doc.revision(), state.cursor);
    match &state.tag_cache {
        Some((revision, pos, tags)) if (*revision, *pos) == key => tags.clone(),
        _ => {
            let tags = transform::matching_tags(doc.text(), state.cursor);
            state.tag_cache = Some((key.0, key.1, tags.clone()));
            tags
        }
    }
}

/// The selected text, if it is a single word whose other occurrences should be highlighted.
fn smart_word(doc: &Document, state: &EditorState) -> Option<String> {
    let selection = state.selection();
    if selection.is_empty() || selection.len() > MAX_SMART_WORD || state.has_multiple_selections() {
        return None;
    }
    let word = doc.slice(selection);
    word.chars()
        .all(|c| c.is_alphanumeric() || c == '_')
        .then_some(word)
}

/// Char ranges of whole-word occurrences of `word` in `text`.
fn word_occurrences(text: &str, word: &str) -> Vec<Range<usize>> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let len = word.chars().count();
    text.match_indices(word)
        .filter(|&(byte, _)| {
            let before = text[..byte].chars().next_back();
            let after = text[byte + word.len()..].chars().next();
            !before.is_some_and(is_word) && !after.is_some_and(is_word)
        })
        .map(|(byte, _)| {
            let start = text[..byte].chars().count();
            start..start + len
        })
        .collect()
}

/// Whether `event` would change the text.
fn is_edit(event: &Event) -> bool {
    match event {
        Event::Text(_) | Event::Cut | Event::Paste(_) | Event::Ime(_) => true,
        Event::Key {
            key,
            modifiers,
            pressed: true,
            ..
        } => {
            matches!(key, Key::Backspace | Key::Delete | Key::Enter | Key::Tab)
                || (modifiers.command && matches!(key, Key::Z | Key::Y))
        }
        _ => false,
    }
}

/// Whether a macro records `event`.
fn is_recordable(event: &Event) -> bool {
    match event {
        Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_) => true,
        Event::Ime(ImeEvent::Commit(_)) => true,
        Event::Key { pressed, .. } => *pressed,
        _ => false,
    }
}

struct Layouter {
    ctx: egui::Context,
    font: FontId,
    palette: Palette,
    tab_width: usize,
    char_width: f32,
    /// Columns to wrap at, if word wrap is on.
    wrap_width: Option<usize>,
    /// Visible display columns, used to window long lines.
    visible_columns: Range<usize>,
    /// Wrap segments of long lines, valid for `segment_revision`.
    long_segments: RefCell<SegmentCache>,
    segment_revision: Cell<u64>,
}

impl Layouter {
    /// Visual rows of `line`, as char column ranges.
    fn segments(&self, rope: &Rope, line: usize) -> Arc<[Range<usize>]> {
        let len = line_len(rope, line);
        let Some(width) = self.wrap_width else {
            return std::iter::once(0..len).collect();
        };
        let compute = || {
            if len > HUGE_LINE {
                // Breaking huge lines at words would walk all their chars on every edit.
                return (0..len)
                    .step_by(width)
                    .map(|start| start..(start + width).min(len))
                    .collect();
            }
            let points = wrap_points(rope.line(line).chars(), self.tab_width, width);
            points
                .iter()
                .zip(points.iter().skip(1).chain([&len]))
                .map(|(&start, &end)| start..end)
                .collect()
        };
        if len <= LONG_LINE {
            return compute();
        }
        let mut cache = self.long_segments.borrow_mut();
        let key = (self.segment_revision.get(), width, self.tab_width);
        if cache.key != key {
            cache.key = key;
            cache.lines.clear();
        }
        cache
            .lines
            .entry((line, len))
            .or_insert_with(compute)
            .clone()
    }

    /// Index of the segment that shows column `col` (a segment end belongs to the next row).
    fn segment_of(segments: &[Range<usize>], col: usize) -> usize {
        segments
            .iter()
            .rposition(|segment| segment.start <= col)
            .unwrap_or(0)
    }

    /// The row map for spacers, folds and wrapped lines, using a cached wrap pass when possible.
    fn row_map(
        &self,
        doc: &Document,
        state: &mut EditorState,
        spacers: &[(usize, usize)],
    ) -> Arc<RowMap> {
        self.segment_revision.set(doc.revision());
        let mut hasher = std::hash::DefaultHasher::new();
        spacers.hash(&mut hasher);
        let key = RowKey {
            revision: doc.revision(),
            wrap_width: self.wrap_width,
            tab_width: self.tab_width,
            spacers: hasher.finish(),
            folds: fold_fingerprint(state),
        };
        if let Some((cached, rows)) = &state.row_cache
            && *cached == key
        {
            return rows.clone();
        }
        let folds: Vec<(usize, usize)> = state.folds().iter().map(|(&h, &n)| (h, n)).collect();
        let rows = Arc::new(match self.wrap_width {
            None => RowMap::new(spacers, &[], &folds),
            Some(width) => {
                let extra_rows = self.wrap(doc, state, width);
                RowMap::new(spacers, extra_rows, &folds)
            }
        });
        state.row_cache = Some((key, rows.clone()));
        rows
    }

    /// Extra rows of wrapped lines, from the cache or updated for the lines that changed.
    fn wrap<'s>(
        &self,
        doc: &Document,
        state: &'s mut EditorState,
        width: usize,
    ) -> &'s [(usize, usize)] {
        let rope = doc.text();
        let lines = rope.len_lines();
        let cache = state
            .wrap_cache
            .take()
            .filter(|cache| (cache.width, cache.tab_width) == (width, self.tab_width));
        let extra_rows = match cache {
            Some(cache) if cache.revision == doc.revision() => cache.extra_rows,
            Some(cache) => match doc.unchanged_since(cache.revision) {
                Some((head, tail)) if head > 0 || tail > 0 => {
                    self.update_extra_rows(rope, cache, head, tail)
                }
                _ => self.extra_rows_in(rope, 0..lines),
            },
            None => self.extra_rows_in(rope, 0..lines),
        };
        let cache = state.wrap_cache.insert(WrapCache {
            revision: doc.revision(),
            width,
            tab_width: self.tab_width,
            lines,
            extra_rows,
        });
        &cache.extra_rows
    }

    /// `(line, extra rows)` for the wrapping lines among `lines`.
    fn extra_rows_in(&self, rope: &Rope, lines: Range<usize>) -> Vec<(usize, usize)> {
        let width = self.wrap_width.unwrap_or(usize::MAX);
        lines
            .filter_map(|line| {
                let slice = rope.line(line);
                // Most lines are short and plain; skip them without walking their chars.
                let simple = slice.len_chars() <= width
                    && slice
                        .chunks()
                        .all(|chunk| !chunk.contains(|c: char| c == '\t' || c < ' ' && c != '\n'));
                if simple {
                    return None;
                }
                let rows = self.segments(rope, line).len();
                (rows > 1).then_some((line, rows - 1))
            })
            .collect()
    }

    /// The extra rows of `cache` after edits that left `head` chars at the start and `tail`
    /// chars at the end unchanged: only the lines in between are wrapped again.
    fn update_extra_rows(
        &self,
        rope: &Rope,
        cache: WrapCache,
        head: usize,
        tail: usize,
    ) -> Vec<(usize, usize)> {
        let len = rope.len_chars();
        let lines = rope.len_lines();
        let first = rope.char_to_line(head.min(len));
        let last = rope.char_to_line(len - tail.min(len));
        let delta = lines as isize - cache.lines as isize;
        let Some(old_last) = last.checked_add_signed(-delta).filter(|&l| l < cache.lines) else {
            return self.extra_rows_in(rope, 0..lines);
        };
        let mut rows: Vec<(usize, usize)> = cache
            .extra_rows
            .iter()
            .copied()
            .take_while(|&(line, _)| line < first)
            .collect();
        rows.extend(self.extra_rows_in(rope, first..(last + 1).min(lines)));
        rows.extend(
            cache
                .extra_rows
                .iter()
                .filter(|&&(line, _)| line > old_last)
                .map(|&(line, extra)| (line.saturating_add_signed(delta), extra)),
        );
        rows
    }

    /// The shown part of a visual row of `line`: first column, its display column and the
    /// number of chars.
    fn window(&self, rope: &Rope, line: usize, segment: &Range<usize>) -> (usize, usize, usize) {
        let len = line_len(rope, line);
        if self.wrap_width.is_some() {
            (segment.start, 0, segment.len())
        } else if len > LONG_LINE {
            let target = self.visible_columns.start.saturating_sub(LONG_LINE_MARGIN);
            let (col, display) = column_at_display(rope.line(line), target, self.tab_width);
            (
                col,
                display,
                self.visible_columns.len() + 2 * LONG_LINE_MARGIN,
            )
        } else {
            (0, 0, len)
        }
    }

    /// Lays out one visual row of `line`: the segment if wrapping, else the visible window.
    fn view(&self, rope: &Rope, line: usize, segment: &Range<usize>, tokens: &[Token]) -> LineView {
        let slice = rope.line(line);
        let len = line_len(rope, line);
        let (first_col, start_display, count) = self.window(rope, line, segment);
        let chars = slice.chars_at(first_col).take(count.min(len - first_col));
        let (text, columns) = expand_tabs(chars, self.tab_width, start_display);
        let galley = if tokens.is_empty() {
            self.text(text)
        } else {
            self.colored(text, &columns, first_col, tokens)
        };
        LineView {
            end_display: start_display + columns.last().copied().unwrap_or(0),
            galley,
            columns,
            first_col,
            // Exact for monospace text, which is all a window is used for.
            origin_x: start_display as f32 * self.char_width,
        }
    }

    fn text(&self, text: String) -> Arc<Galley> {
        self.ctx
            .fonts_mut(|f| f.layout_no_wrap(text, self.font.clone(), self.palette.text))
    }

    fn colored(
        &self,
        text: String,
        columns: &[usize],
        first_col: usize,
        tokens: &[Token],
    ) -> Arc<Galley> {
        let bytes: Vec<usize> = text
            .char_indices()
            .map(|(byte, _)| byte)
            .chain([text.len()])
            .collect();
        let last = columns.len() - 1;
        let display = |col: usize| columns[col.saturating_sub(first_col).min(last)];
        let mut job = LayoutJob::default();
        let add = |job: &mut LayoutJob, from: usize, to: usize, color: Color32| {
            if from < to {
                let format = TextFormat::simple(self.font.clone(), color);
                job.append(&text[bytes[from]..bytes[to]], 0.0, format);
            }
        };
        let mut pos = 0;
        for token in tokens {
            let end = display(token.end);
            if end <= pos {
                continue;
            }
            let start = display(token.start).max(pos);
            add(&mut job, pos, start, self.palette.text);
            add(&mut job, start, end, self.palette.color(Some(token.kind)));
            pos = end;
        }
        add(&mut job, pos, bytes.len() - 1, self.palette.text);
        if job.text.is_empty() {
            return self.text(String::new());
        }
        self.ctx.fonts_mut(|f| f.layout_job(job))
    }
}

/// A laid-out visual row: a wrapped segment, or for long lines a window starting at
/// `first_col`, with the mapping from document columns to display chars.
struct LineView {
    galley: Arc<Galley>,
    columns: Vec<usize>,
    first_col: usize,
    origin_x: f32,
    end_display: usize,
}

impl LineView {
    fn x(&self, col: usize) -> f32 {
        self.origin_x + self.local_x(col.saturating_sub(self.first_col))
    }

    fn local_x(&self, index: usize) -> f32 {
        let display = self.columns[index.min(self.columns.len() - 1)];
        self.galley.pos_from_cursor(CCursor::new(display)).min.x
    }

    fn col_at(&self, x: f32) -> usize {
        let x = x - self.origin_x;
        let display = self.galley.cursor_from_pos(vec2(x, 0.0)).index.0;
        let i = self.columns.partition_point(|&d| d < display);
        if i == 0 || (i < self.columns.len() && self.columns[i] == display) {
            return self.first_col + i;
        }
        // Inside an expanded tab: snap to the nearer edge.
        let i = i.min(self.columns.len() - 1);
        let nearer = if x - self.local_x(i - 1) <= self.local_x(i) - x {
            i - 1
        } else {
            i
        };
        self.first_col + nearer
    }

    /// Horizontal extent of `range` (char indices) within this row of the line starting at
    /// `line_start` and ending at `row_end`; `line_break` marks the line's last row.
    fn span(
        &self,
        range: &Range<usize>,
        line_start: usize,
        row_end: usize,
        line_break: bool,
    ) -> Range<f32> {
        let row_start = line_start + self.first_col;
        let from = self.x(range.start.max(row_start) - line_start);
        let mut to = self.x(range.end.min(row_end) - line_start);
        if line_break && range.end > row_end {
            // Show that the line break is included.
            to += (self.local_x(1) - self.local_x(0)).max(4.0);
        }
        from..to
    }
}

#[derive(Clone, Copy)]
struct Geometry<'a> {
    origin: Pos2,
    text_left: f32,
    /// Screen x where the (non-scrolling) gutter ends.
    gutter_right: f32,
    fold_width: f32,
    row_height: f32,
    rows: &'a RowMap,
}

impl Geometry<'_> {
    fn row_top(&self, row: usize) -> f32 {
        self.origin.y + row as f32 * self.row_height
    }
}

struct Input<'a> {
    doc: &'a mut Document,
    state: &'a mut EditorState,
    layouter: &'a Layouter,
    rows: &'a RowMap,
    mac: bool,
    page_rows: usize,
    indent_with_spaces: bool,
    auto_close: bool,
    read_only: bool,
    folding: bool,
}

impl Input<'_> {
    fn pos_at(&self, geometry: &Geometry, point: Pos2) -> usize {
        let rope = self.doc.text();
        let row = ((point.y - geometry.origin.y) / geometry.row_height).floor();
        if row < 0.0 {
            return 0;
        }
        let (line, kind) = geometry.rows.line_at(row as usize);
        if line >= rope.len_lines() {
            return rope.len_chars();
        }
        let RowKind::Line(sub) = kind else {
            return rope.line_to_char(line);
        };
        let segments = self.layouter.segments(rope, line);
        let segment = &segments[sub.min(segments.len() - 1)];
        let col = self
            .layouter
            .view(rope, line, segment, &[])
            .col_at(point.x - geometry.text_left);
        rope.line_to_char(line) + clamp_to_row(col, segment, sub + 1 == segments.len())
    }

    fn handle_pointer(&mut self, ui: &Ui, response: &Response, geometry: &Geometry) {
        if !response.is_pointer_button_down_on() {
            self.state.column_start = None;
        }
        let Some(point) = response.interact_pointer_pos() else {
            return;
        };
        let pos = self.pos_at(geometry, point);
        let rope = self.doc.text();
        let modifiers = ui.input(|i| i.modifiers);
        let pressed =
            response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed());
        let tab_width = self.layouter.tab_width;

        if pressed && point.x < geometry.gutter_right {
            let line = rope.char_to_line(pos);
            if self.folding && point.x >= geometry.gutter_right - geometry.fold_width {
                self.state.toggle_fold(self.doc, line, tab_width);
            } else {
                self.state.toggle_bookmark(line);
            }
            response.request_focus();
            return;
        }
        let (char_width, text_left) = (self.layouter.char_width, geometry.text_left);
        let column = |x: f32| ((x - text_left) / char_width).round().max(0.0) as usize;
        if pressed && modifiers.alt && !modifiers.shift {
            response.request_focus();
            self.state.column_start = Some((rope.char_to_line(pos), column(point.x)));
            self.state.extra.clear();
            self.move_to(pos, false);
            return;
        }
        if let Some(start) = self.state.column_start {
            if response.dragged() {
                let end = (rope.char_to_line(pos), column(point.x));
                self.state.column_select(self.doc, start, end, tab_width);
                self.doc.seal_undo_group();
            }
            return;
        }
        let add_cursor = if self.mac {
            modifiers.mac_cmd
        } else {
            modifiers.ctrl
        };
        if pressed && add_cursor {
            response.request_focus();
            self.state.add_cursor(pos);
            self.doc.seal_undo_group();
            return;
        }
        if response.triple_clicked() {
            let line = rope.char_to_line(pos);
            let end = (rope.line_to_char(line) + rope.line(line).len_chars()).min(rope.len_chars());
            self.select(rope.line_to_char(line), end);
        } else if response.double_clicked() {
            let word = word_at(rope, pos);
            self.select(word.start, word.end);
        } else if pressed {
            response.request_focus();
            self.state.extra.clear();
            let extend = modifiers.shift;
            self.move_to(pos, extend);
        } else if response.dragged() {
            self.move_to(pos, true);
        }
    }

    /// Plays back a requested macro.
    fn replay(&mut self, ctx: &egui::Context) {
        let Some((events, repeat)) = self.state.playback.take() else {
            return;
        };
        if events.is_empty() {
            return;
        }
        let times = match repeat {
            Repeat::Times(times) => times,
            Repeat::UntilEnd => MAX_REPLAYS,
        };
        for _ in 0..times {
            let before = self.state.cursor;
            for event in &events {
                self.handle_event(ctx, event);
            }
            let stuck =
                self.state.cursor <= before || self.state.cursor >= self.doc.text().len_chars();
            if repeat == Repeat::UntilEnd && stuck {
                break;
            }
        }
        self.doc.seal_undo_group();
    }

    fn select(&mut self, anchor: usize, cursor: usize) {
        self.state.set_selection(anchor, cursor);
        self.doc.seal_undo_group();
    }

    fn move_to(&mut self, pos: usize, extend: bool) {
        self.state.cursor = pos;
        if !extend {
            self.state.anchor = pos;
        }
        self.state.preferred_x = None;
        self.state.scroll_to_cursor = true;
        self.doc.seal_undo_group();
    }

    /// Moves by visual rows, keeping the horizontal position.
    fn move_vertically(&mut self, delta: isize, extend: bool) {
        let rope = self.doc.text();
        let (line, col) = line_col(rope, self.state.cursor);
        let segments = self.layouter.segments(rope, line);
        let sub = Layouter::segment_of(&segments, col);
        let x = self
            .state
            .preferred_x
            .unwrap_or_else(|| self.layouter.view(rope, line, &segments[sub], &[]).x(col));
        let row = self.rows.row(line) + sub;
        let total = self.rows.total_rows(rope.len_lines());
        let mut target = row.saturating_add_signed(delta);
        let pos = if delta < 0 && row == 0 {
            0
        } else if target >= total {
            rope.len_chars()
        } else {
            // Spacer rows hold no text; step over them in the direction of travel.
            let mut found = self.rows.line_at(target);
            while found.1 == RowKind::Spacer {
                target = if delta < 0 {
                    target.saturating_sub(1)
                } else {
                    target + 1
                };
                if target == 0 || target >= total {
                    break;
                }
                found = self.rows.line_at(target);
            }
            let (target_line, kind) = found;
            if target_line >= rope.len_lines() {
                rope.len_chars()
            } else {
                let segments = self.layouter.segments(rope, target_line);
                let sub = match kind {
                    RowKind::Line(sub) => sub.min(segments.len() - 1),
                    RowKind::Spacer => 0,
                };
                let segment = &segments[sub];
                let col = self
                    .layouter
                    .view(rope, target_line, segment, &[])
                    .col_at(x);
                rope.line_to_char(target_line)
                    + clamp_to_row(col, segment, sub + 1 == segments.len())
            }
        };
        self.move_to(pos, extend);
        self.state.preferred_x = Some(x);
    }

    fn insert(&mut self, text: &str) {
        self.state.insert(self.doc, text);
    }

    fn delete(&mut self, start: usize, end: usize) {
        if start < end {
            self.doc.replace(start..end, "");
            self.state.set_cursor(start);
        }
    }

    fn handle_event(&mut self, ctx: &egui::Context, event: &Event) {
        if self.read_only && is_edit(event) {
            return;
        }
        if !self.state.extra.is_empty() && self.handle_multi(ctx, event) {
            return;
        }
        match event {
            Event::Text(text) if text != "\n" && text != "\r" => self.type_text(text),
            Event::Copy => self.state.copy(self.doc, ctx),
            Event::Cut => self.state.cut(self.doc, ctx),
            Event::Paste(text) if !text.is_empty() => {
                self.doc.seal_undo_group();
                self.insert(text);
                self.doc.seal_undo_group();
            }
            Event::Ime(ime) => self.handle_ime(ime),
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => self.handle_key(*key, *modifiers),
            _ => {}
        }
    }

    /// Handles `event` for several selections at once. Returns whether it was handled; other
    /// keys go back to a single cursor.
    fn handle_multi(&mut self, ctx: &egui::Context, event: &Event) -> bool {
        match event {
            Event::Text(text) | Event::Ime(ImeEvent::Commit(text))
                if !text.is_empty() && text != "\n" && text != "\r" =>
            {
                self.state.preedit.clear();
                self.state
                    .replace_each(self.doc, |_, range, _| (range, text.clone()));
            }
            Event::Ime(ImeEvent::Preedit { text, .. }) => self.state.preedit.clone_from(text),
            Event::Copy => self.state.copy(self.doc, ctx),
            Event::Cut => self.state.cut(self.doc, ctx),
            Event::Paste(text) if !text.is_empty() => {
                let count = self.state.extra.len() + 1;
                let text = text.replace("\r\n", "\n");
                let lines: Vec<&str> = text
                    .strip_suffix('\n')
                    .unwrap_or(&text)
                    .split('\n')
                    .collect();
                let mut index = 0;
                self.state.replace_each(self.doc, |_, range, _| {
                    index += 1;
                    let piece = if lines.len() == count {
                        lines[index - 1].to_owned()
                    } else {
                        text.clone()
                    };
                    (range, piece)
                });
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if !self.multi_key(*key, *modifiers) {
                    self.state.extra.clear();
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    /// Keys for several selections; returns whether `key` was handled.
    fn multi_key(&mut self, key: Key, modifiers: Modifiers) -> bool {
        let extend = modifiers.shift;
        let word = if self.mac {
            modifiers.alt
        } else {
            modifiers.ctrl
        };
        let mac_cmd = self.mac && modifiers.mac_cmd;
        let tab_width = self.layouter.tab_width;
        match key {
            Key::Escape => self.state.extra.clear(),
            Key::ArrowLeft | Key::ArrowRight => {
                let right = key == Key::ArrowRight;
                self.move_each(extend, |rope, cursor, selection| {
                    let (line, _) = line_col(rope, cursor);
                    if !extend && !word && !mac_cmd && !selection.is_empty() {
                        if right {
                            selection.end
                        } else {
                            selection.start
                        }
                    } else if mac_cmd {
                        if right {
                            line_end(rope, line)
                        } else {
                            smart_home(rope, cursor)
                        }
                    } else if word {
                        if right {
                            next_word(rope, cursor)
                        } else {
                            prev_word(rope, cursor)
                        }
                    } else if right {
                        (cursor + 1).min(rope.len_chars())
                    } else {
                        cursor.saturating_sub(1)
                    }
                });
            }
            Key::ArrowUp | Key::ArrowDown if !mac_cmd && !modifiers.command => {
                let down = key == Key::ArrowDown;
                self.move_each(extend, |rope, cursor, _| {
                    let (line, col) = line_col(rope, cursor);
                    let target = if down {
                        line + 1
                    } else if line == 0 {
                        return 0;
                    } else {
                        line - 1
                    };
                    if target >= rope.len_lines() {
                        return rope.len_chars();
                    }
                    let display = display_column(rope.line(line), col, tab_width);
                    let (col, _) = column_at_display(rope.line(target), display, tab_width);
                    rope.line_to_char(target) + col.min(line_len(rope, target))
                });
            }
            Key::Home if !modifiers.command => {
                self.move_each(extend, |rope, cursor, _| smart_home(rope, cursor));
            }
            Key::End if !modifiers.command => {
                self.move_each(extend, |rope, cursor, _| {
                    line_end(rope, rope.char_to_line(cursor))
                });
            }
            Key::Backspace | Key::Delete => {
                let forward = key == Key::Delete;
                self.state.replace_each(self.doc, |rope, range, cursor| {
                    if !range.is_empty() {
                        return (range, String::new());
                    }
                    let (line, _) = line_col(rope, cursor);
                    let range = match (forward, mac_cmd, word) {
                        (false, true, _) => rope.line_to_char(line)..cursor,
                        (false, _, true) => prev_word(rope, cursor)..cursor,
                        (false, _, _) => cursor.saturating_sub(1)..cursor,
                        (true, _, true) => cursor..next_word(rope, cursor),
                        (true, _, _) => cursor..(cursor + 1).min(rope.len_chars()),
                    };
                    (range, String::new())
                });
            }
            Key::Enter => {
                self.state.replace_each(self.doc, |rope, range, _| {
                    let line = rope.char_to_line(range.start);
                    let mut indent = leading_whitespace(rope.line(line));
                    indent.truncate(range.start - rope.line_to_char(line));
                    (range, format!("\n{indent}"))
                });
            }
            Key::Tab if !extend => {
                let spaces = self.indent_with_spaces;
                self.state.replace_each(self.doc, |rope, range, _| {
                    let (line, col) = line_col(rope, range.start);
                    let unit = if spaces {
                        let display = display_column(rope.line(line), col, tab_width);
                        " ".repeat(tab_width - display % tab_width)
                    } else {
                        "\t".to_owned()
                    };
                    (range, unit)
                });
            }
            _ => return false,
        }
        true
    }

    /// Moves every cursor to where `target` says; with `extend`, selections grow.
    fn move_each(&mut self, extend: bool, target: impl Fn(&Rope, usize, Range<usize>) -> usize) {
        let (all, primary) = self.state.all_sorted();
        let rope = self.doc.text();
        let moved = all
            .into_iter()
            .map(|(anchor, cursor)| {
                let pos = target(rope, cursor, anchor.min(cursor)..anchor.max(cursor));
                if extend { (anchor, pos) } else { (pos, pos) }
            })
            .collect();
        self.state.set_all(moved, primary);
        self.doc.seal_undo_group();
    }

    /// Inserts typed text, closing brackets and quotes if enabled.
    fn type_text(&mut self, text: &str) {
        let mut chars = text.chars();
        let (Some(c), None, true) = (chars.next(), chars.next(), self.auto_close) else {
            self.insert(text);
            return;
        };
        let rope = self.doc.text();
        let cursor = self.state.cursor;
        let selection = self.state.selection();
        let next = (cursor < rope.len_chars()).then(|| rope.char(cursor));
        let prev = cursor.checked_sub(1).map(|p| rope.char(p));
        let close = match c {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '"' | '\'' => c,
            ')' | ']' | '}' if selection.is_empty() && next == Some(c) => {
                self.move_to(cursor + 1, false);
                return;
            }
            _ => {
                self.insert(text);
                return;
            }
        };
        let quote = close == c;
        if quote && selection.is_empty() && next == Some(c) {
            self.move_to(cursor + 1, false);
            return;
        }
        // An apostrophe inside a word ("don't") is not an opening quote.
        if quote && selection.is_empty() && prev.is_some_and(char::is_alphanumeric) {
            self.insert(text);
            return;
        }
        self.doc.seal_undo_group();
        if selection.is_empty() {
            self.insert(&format!("{c}{close}"));
            let inside = self.state.cursor - 1;
            self.state.set_cursor(inside);
        } else {
            let inner = self.state.selected_text(self.doc);
            self.insert(&format!("{c}{inner}{close}"));
            let end = self.state.cursor - 1;
            self.state.set_selection(selection.start + 1, end);
        }
        self.doc.seal_undo_group();
    }

    fn handle_ime(&mut self, ime: &ImeEvent) {
        match ime {
            ImeEvent::Preedit { text, .. } => self.state.preedit.clone_from(text),
            ImeEvent::Commit(text) => {
                self.state.preedit.clear();
                if !text.is_empty() && text != "\n" && text != "\r" {
                    self.insert(text);
                }
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key: Key, modifiers: Modifiers) {
        let extend = modifiers.shift;
        let word = if self.mac {
            modifiers.alt
        } else {
            modifiers.ctrl
        };
        let mac_cmd = self.mac && modifiers.mac_cmd;
        let rope = self.doc.text();
        let cursor = self.state.cursor;
        let selection = self.state.selection();
        let (line, _) = line_col(rope, cursor);
        let line_start = rope.line_to_char(line);

        match key {
            Key::ArrowLeft if !extend && !word && !mac_cmd && !selection.is_empty() => {
                self.move_to(selection.start, false);
            }
            Key::ArrowRight if !extend && !word && !mac_cmd && !selection.is_empty() => {
                self.move_to(selection.end, false);
            }
            Key::ArrowLeft => {
                let pos = if mac_cmd {
                    smart_home(rope, cursor)
                } else if word {
                    prev_word(rope, cursor)
                } else {
                    cursor.saturating_sub(1)
                };
                self.move_to(pos, extend);
            }
            Key::ArrowRight => {
                let pos = if mac_cmd {
                    line_end(rope, line)
                } else if word {
                    next_word(rope, cursor)
                } else {
                    (cursor + 1).min(rope.len_chars())
                };
                self.move_to(pos, extend);
            }
            Key::ArrowUp if mac_cmd => self.move_to(0, extend),
            Key::ArrowDown if mac_cmd => self.move_to(rope.len_chars(), extend),
            Key::ArrowUp => self.move_vertically(-1, extend),
            Key::ArrowDown => self.move_vertically(1, extend),
            Key::PageUp => self.move_vertically(-(self.page_rows as isize), extend),
            Key::PageDown => self.move_vertically(self.page_rows as isize, extend),
            Key::Home if modifiers.command => self.move_to(0, extend),
            Key::End if modifiers.command => self.move_to(rope.len_chars(), extend),
            Key::Home => self.move_to(smart_home(rope, cursor), extend),
            Key::End => self.move_to(line_end(rope, line), extend),

            Key::Backspace if !selection.is_empty() => self.delete(selection.start, selection.end),
            Key::Delete if !selection.is_empty() => self.delete(selection.start, selection.end),
            Key::Backspace => {
                let empty_pair = self.auto_close
                    && cursor > 0
                    && cursor < rope.len_chars()
                    && matches!(
                        (rope.char(cursor - 1), rope.char(cursor)),
                        ('(', ')') | ('[', ']') | ('{', '}') | ('"', '"') | ('\'', '\'')
                    );
                let start = if mac_cmd {
                    line_start
                } else if word {
                    prev_word(rope, cursor)
                } else {
                    cursor.saturating_sub(1)
                };
                let end = if empty_pair && !word && !mac_cmd {
                    cursor + 1
                } else {
                    cursor
                };
                self.delete(start, end);
            }
            Key::Delete => {
                let end = if word {
                    next_word(rope, cursor)
                } else {
                    (cursor + 1).min(rope.len_chars())
                };
                self.delete(cursor, end);
            }
            Key::Enter => {
                let mut indent = leading_whitespace(rope.line(line));
                indent.truncate(cursor - line_start);
                self.doc.seal_undo_group();
                self.insert(&format!("\n{indent}"));
            }
            Key::Tab if extend => self.unindent(),
            Key::Tab if ops::selected_lines(self.doc, &selection).len() > 1 => {
                let unit = self.indent_unit(None);
                let range = ops::indent_lines(self.doc, &selection, &unit);
                self.state.set_selection(range.start, range.end);
            }
            Key::Tab => {
                let (line, col) = line_col(rope, cursor);
                let unit = self.indent_unit(Some((line, col)));
                self.insert(&unit);
            }

            Key::A if modifiers.command => self.state.select_all(self.doc),
            Key::Z if modifiers.command && modifiers.shift => self.state.redo(self.doc),
            Key::Z if modifiers.command => self.state.undo(self.doc),
            Key::Y if modifiers.command && !self.mac => self.state.redo(self.doc),
            _ => {}
        }
    }

    /// One indentation step; with spaces and a position, only up to the next tab stop.
    fn indent_unit(&self, at: Option<(usize, usize)>) -> String {
        let tab_width = self.layouter.tab_width;
        if !self.indent_with_spaces {
            return "\t".into();
        }
        let display = at.map_or(0, |(line, col)| {
            display_column(self.doc.text().line(line), col, tab_width)
        });
        " ".repeat(tab_width - display % tab_width)
    }

    fn unindent(&mut self) {
        let selection = self.state.selection();
        let multi_line = ops::selected_lines(self.doc, &selection).len() > 1;
        let before = self.doc.text().len_chars();
        let range = ops::unindent_lines(self.doc, &selection, self.layouter.tab_width);
        if multi_line {
            self.state.set_selection(range.start, range.end);
        } else {
            let removed = before - self.doc.text().len_chars();
            let line_start = self
                .doc
                .text()
                .line_to_char(self.doc.text().char_to_line(range.start));
            self.state
                .set_cursor(self.state.cursor.saturating_sub(removed).max(line_start));
        }
    }
}

/// Keeps `col` on its visual row: the end of a row that is not the line's last belongs to
/// the next row.
fn clamp_to_row(col: usize, segment: &Range<usize>, last: bool) -> usize {
    if last || segment.is_empty() {
        col.min(segment.end)
    } else {
        col.min(segment.end - 1)
    }
}

struct View<'a> {
    doc: &'a Document,
    state: &'a mut EditorState,
    layouter: &'a Layouter,
    geometry: Geometry<'a>,
    viewport: Rect,
    gutter_width: f32,
    has_focus: bool,
    highlights: &'a [Range<usize>],
    spans: &'a [(Range<usize>, Color32)],
    line_backgrounds: &'a [(Range<usize>, Color32)],
    gutter_marks: &'a [(Range<usize>, Color32)],
    highlighter: Option<&'a Highlighter>,
    brackets: Option<Brackets>,
    tags: Option<(Range<usize>, Range<usize>)>,
    smart_word: Option<String>,
    folding: bool,
    show_whitespace: bool,
    show_line_endings: bool,
    show_wrap_symbol: bool,
    indent_guides: bool,
    edge_column: Option<usize>,
}

impl View<'_> {
    /// Screen rectangle of the char position `pos` (a thin cursor-sized box).
    fn pos_rect(&self, pos: usize) -> Rect {
        let rope = self.doc.text();
        let (line, col) = line_col(rope, pos.min(rope.len_chars()));
        let segments = self.layouter.segments(rope, line);
        let sub = Layouter::segment_of(&segments, col);
        let view = self.layouter.view(rope, line, &segments[sub], &[]);
        let top = self.geometry.row_top(self.geometry.rows.row(line) + sub);
        Rect::from_min_size(
            pos2(self.geometry.text_left + view.x(col), top),
            vec2(CURSOR_WIDTH, self.geometry.row_height),
        )
    }

    /// Paints the visible rows. Returns the widest visible line in display columns and the
    /// visible lines.
    fn paint(&self, ui: &Ui, cursor_rect: Rect) -> (usize, Range<usize>) {
        let rope = self.doc.text();
        let visuals = ui.visuals();
        let palette = &self.layouter.palette;
        let painter = ui.painter();
        let Geometry {
            origin,
            text_left,
            row_height,
            rows,
            ..
        } = self.geometry;

        let line_count = rope.len_lines();
        let first_row = (self.viewport.min.y / row_height).floor().max(0.0) as usize;
        let last_row = (self.viewport.max.y / row_height).ceil() as usize;
        let first = rows.line_at(first_row).0.min(line_count);
        let last = (rows.line_at(last_row).0 + 1).min(line_count);
        let visible_rows = first_row..last_row + 1;
        let selection = self.state.selection();
        let extra = &self.state.extra;
        let cursor_line = rope.char_to_line(self.state.cursor);
        let visible_left = origin.x + self.viewport.min.x;
        let visible_right = origin.x + self.viewport.max.x;
        let full_row = |top: f32, count: usize| {
            Rect::from_x_y_ranges(
                visible_left..=visible_right,
                top..=top + count as f32 * row_height,
            )
        };

        painter.rect_filled(
            self.viewport.translate(origin.to_vec2()),
            0.0,
            palette.background,
        );
        for (row, count) in rows.blocks(first..last + 1) {
            painter.rect_filled(
                full_row(self.geometry.row_top(row), count),
                0.0,
                palette.current_line.gamma_multiply(2.0),
            );
        }
        if let Some(column) = self.edge_column {
            let x = text_left + column as f32 * self.layouter.char_width;
            let y = origin.y + self.viewport.min.y..=origin.y + self.viewport.max.y;
            painter.vline(x, y, Stroke::new(1.0, palette.faint()));
        }

        let mut widest = 0;
        let highlight_fill = visuals.warn_fg_color.gamma_multiply(0.35);
        let smart_fill = Color32::from_rgba_unmultiplied(40, 200, 90, 70);
        let mut line = first;
        while line < last {
            let hidden = rows.hidden_after(line);
            let huge = line_len(rope, line) > HUGE_LINE;
            let mut tokens = match self.highlighter {
                Some(highlighter) if !huge => highlighter.tokens(self.doc, line),
                _ => Vec::new(),
            };
            let segments = self.layouter.segments(rope, line);
            let line_start = rope.line_to_char(line);
            let first_line_row = rows.row(line);
            let background = {
                let index = self
                    .line_backgrounds
                    .partition_point(|(r, _)| r.end <= line);
                self.line_backgrounds
                    .get(index)
                    .filter(|(range, _)| range.contains(&line))
                    .map(|(_, color)| *color)
            };
            let smart: Vec<Range<usize>> = match &self.smart_word {
                Some(word) if line_len(rope, line) <= LONG_LINE => {
                    let text: String = rope.line(line).chars().collect();
                    word_occurrences(&text, word)
                        .into_iter()
                        .map(|r| line_start + r.start..line_start + r.end)
                        .collect()
                }
                _ => Vec::new(),
            };

            for (sub, segment) in segments.iter().enumerate() {
                if !visible_rows.contains(&(first_line_row + sub)) {
                    continue;
                }
                let top = self.geometry.row_top(first_line_row + sub);
                let row_y = top..=top + row_height;
                let last_segment = sub + 1 == segments.len();
                let row_start = line_start + segment.start;
                let row_end = line_start + segment.end;
                if huge && let Some(highlighter) = self.highlighter {
                    let (first_col, _, count) = self.layouter.window(rope, line, segment);
                    tokens = highlighter.tokens_in(self.doc, line, first_col..first_col + count);
                }
                let view = self.layouter.view(rope, line, segment, &tokens);
                widest = widest.max(view.end_display);

                if let Some(color) = background {
                    painter.rect_filled(full_row(top, 1), 0.0, color);
                }
                if line == cursor_line {
                    painter.rect_filled(full_row(top, 1), 0.0, palette.current_line);
                }
                if sub == 0 && self.indent_guides {
                    self.paint_indent_guides(ui, line, top);
                }

                let overlapping = |range: &Range<usize>| {
                    let ends_after_start =
                        range.end > row_start || (range.end == row_start && range.is_empty());
                    let starts_before_end = if last_segment {
                        range.start <= row_end
                    } else {
                        range.start < row_end
                    };
                    ends_after_start && starts_before_end && !range.is_empty()
                };
                let fill = |range: &Range<usize>, color: Color32, rounding: f32| {
                    let span = view.span(range, line_start, row_end, last_segment);
                    let rect = Rect::from_x_y_ranges(
                        text_left + span.start..=text_left + span.end,
                        row_y.clone(),
                    );
                    painter.rect_filled(rect, rounding, color);
                };
                let first_span = self.spans.partition_point(|(r, _)| r.end <= row_start);
                for (range, color) in self.spans[first_span..]
                    .iter()
                    .take_while(|(r, _)| r.start <= row_end)
                    .filter(|(r, _)| overlapping(r))
                {
                    fill(range, *color, 0.0);
                }
                for range in smart.iter().filter(|r| overlapping(r)) {
                    fill(range, smart_fill, 2.0);
                }
                let first_highlight = self.highlights.partition_point(|r| r.end <= row_start);
                for range in self.highlights[first_highlight..]
                    .iter()
                    .take_while(|r| r.start <= row_end)
                    .filter(|r| overlapping(r))
                {
                    fill(range, highlight_fill, 2.0);
                }
                if overlapping(&selection) {
                    fill(&selection, palette.selection, 0.0);
                }
                let first_extra = extra.partition_point(|&(a, c)| a.max(c) < row_start);
                for range in extra[first_extra..]
                    .iter()
                    .map(|&(a, c)| a.min(c)..a.max(c))
                    .take_while(|r| r.start <= row_end)
                    .filter(|r| overlapping(r))
                {
                    fill(&range, palette.selection, 0.0);
                }
                self.paint_brackets(ui, &view, line_start, row_start..row_end, top);
                if let Some((a, b)) = &self.tags {
                    for range in [a, b].into_iter().filter(|r| overlapping(r)) {
                        fill(range, palette.selection.gamma_multiply(0.6), 2.0);
                    }
                }

                painter.galley(
                    pos2(text_left + view.origin_x, top),
                    view.galley.clone(),
                    palette.text,
                );
                if self.show_whitespace {
                    self.paint_whitespace(ui, line, &view, top);
                }
                let end_x = text_left + view.x(segment.end) + 2.0;
                if !last_segment && self.show_wrap_symbol {
                    paint_wrap_symbol(ui, pos2(end_x, top + row_height / 2.0), palette.faint());
                }
                if last_segment && self.show_line_endings && line + 1 < line_count {
                    painter.text(
                        pos2(end_x, top),
                        Align2::LEFT_TOP,
                        "¶",
                        self.layouter.font.clone(),
                        palette.faint(),
                    );
                }
                if last_segment && hidden > 0 {
                    let x = end_x + if self.show_line_endings { 12.0 } else { 4.0 };
                    let label = painter.text(
                        pos2(x + 3.0, top),
                        Align2::LEFT_TOP,
                        "…",
                        self.layouter.font.clone(),
                        palette.line_number,
                    );
                    painter.rect_stroke(
                        label.expand2(vec2(3.0, -1.0)),
                        3.0,
                        Stroke::new(1.0, palette.faint()),
                        egui::StrokeKind::Inside,
                    );
                    painter.hline(
                        text_left..=visible_right,
                        top + row_height - 0.5,
                        Stroke::new(1.0, palette.faint()),
                    );
                }
            }
            line += 1 + hidden;
        }

        if self.has_focus {
            if self.state.preedit.is_empty() {
                painter.rect_filled(cursor_rect, 0.0, palette.cursor);
            } else {
                self.paint_preedit(ui, cursor_rect);
            }
            let visible_chars = rope.line_to_char(first)..rope.line_to_char(last);
            let first_extra = extra.partition_point(|&(a, c)| a.max(c) < visible_chars.start);
            for &(_, cursor) in extra[first_extra..]
                .iter()
                .take_while(|&&(a, c)| a.min(c) <= visible_chars.end)
            {
                painter.rect_filled(self.pos_rect(cursor), 0.0, palette.cursor);
            }
        }

        self.paint_gutter(ui, first, last, cursor_line, &visible_rows);
        (widest, first..last)
    }

    /// Highlights the bracket at the cursor and its partner if they are on this row.
    fn paint_brackets(
        &self,
        ui: &Ui,
        view: &LineView,
        line_start: usize,
        row: Range<usize>,
        top: f32,
    ) {
        let Some(brackets) = self.brackets else {
            return;
        };
        let visuals = ui.visuals();
        let (positions, color) = match brackets {
            Brackets::Matched(a, b) => (
                [Some(a), Some(b)],
                visuals.selection.bg_fill.gamma_multiply(0.6),
            ),
            Brackets::Unmatched(a) => ([Some(a), None], visuals.error_fg_color.gamma_multiply(0.4)),
        };
        for pos in positions
            .into_iter()
            .flatten()
            .filter(|pos| row.contains(pos))
        {
            let col = pos - line_start;
            let rect = Rect::from_x_y_ranges(
                self.geometry.text_left + view.x(col)..=self.geometry.text_left + view.x(col + 1),
                top..=top + self.geometry.row_height,
            );
            ui.painter().rect_filled(rect, 2.0, color);
        }
    }

    fn paint_indent_guides(&self, ui: &Ui, line: usize, top: f32) {
        let tab_width = self.layouter.tab_width;
        let slice = self.doc.text().line(line);
        let indent: usize = slice
            .chars()
            .take_while(|&c| c == ' ' || c == '\t')
            .fold(0, |display, c| {
                display + char_columns(c, display, tab_width)
            });
        let stroke = Stroke::new(1.0, self.layouter.palette.faint().gamma_multiply(0.7));
        for level in (tab_width..indent).step_by(tab_width) {
            let x = self.geometry.text_left + level as f32 * self.layouter.char_width;
            ui.painter()
                .vline(x, top..=top + self.geometry.row_height, stroke);
        }
    }

    fn paint_whitespace(&self, ui: &Ui, line: usize, view: &LineView, top: f32) {
        let painter = ui.painter();
        let color = self.layouter.palette.faint();
        let text_left = self.geometry.text_left;
        let middle = top + self.geometry.row_height / 2.0;
        let chars = self
            .doc
            .text()
            .line(line)
            .chars_at(view.first_col)
            .take(view.columns.len() - 1);
        for (index, ch) in chars.enumerate() {
            let col = view.first_col + index;
            let (left, right) = (text_left + view.x(col), text_left + view.x(col + 1));
            match ch {
                ' ' => {
                    painter.circle_filled(pos2((left + right) / 2.0, middle), 1.2, color);
                }
                '\t' => {
                    let stroke = Stroke::new(1.0, color);
                    let tip = pos2(right - 3.0, middle);
                    painter.line_segment([pos2(left + 3.0, middle), tip], stroke);
                    painter.line_segment([tip, tip + vec2(-3.0, -3.0)], stroke);
                    painter.line_segment([tip, tip + vec2(-3.0, 3.0)], stroke);
                }
                _ => {}
            }
        }
    }

    fn paint_preedit(&self, ui: &Ui, cursor_rect: Rect) {
        let palette = &self.layouter.palette;
        let galley = self.layouter.text(self.state.preedit.clone());
        let rect = Rect::from_min_size(cursor_rect.min, galley.size());
        ui.painter().rect_filled(rect, 0.0, palette.background);
        ui.painter().galley(rect.min, galley, palette.text);
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 1.0,
            Stroke::new(1.0, palette.cursor),
        );
    }

    fn paint_gutter(
        &self,
        ui: &Ui,
        first: usize,
        last: usize,
        cursor_line: usize,
        visible_rows: &Range<usize>,
    ) {
        let visuals = ui.visuals();
        let palette = &self.layouter.palette;
        let painter = ui.painter();
        let left = self.geometry.origin.x + self.viewport.min.x;
        let top = self.geometry.origin.y + self.viewport.min.y;
        let gutter = Rect::from_min_size(
            pos2(left, top),
            vec2(self.gutter_width, self.viewport.height()),
        );
        painter.rect_filled(gutter, 0.0, palette.gutter);
        painter.vline(
            gutter.right(),
            gutter.y_range(),
            Stroke::new(1.0, palette.faint().gamma_multiply(0.5)),
        );

        let rope = self.doc.text();
        let rows = self.geometry.rows;
        let row_height = self.geometry.row_height;
        let numbers_right = gutter.right() - self.geometry.fold_width;
        let mut line = first;
        while line < last {
            let hidden = rows.hidden_after(line);
            let row = rows.row(line);
            let current = line;
            line += 1 + hidden;
            if !visible_rows.contains(&row) {
                continue;
            }
            let line = current;
            let top = self.geometry.row_top(row);
            let index = self.gutter_marks.partition_point(|(r, _)| r.end <= line);
            if let Some((range, color)) = self.gutter_marks.get(index)
                && range.contains(&line)
            {
                let rect = Rect::from_x_y_ranges(
                    numbers_right - 4.0..=numbers_right - 1.0,
                    top..=top + row_height,
                );
                painter.rect_filled(rect, 0.0, *color);
            }
            if self.state.bookmarks().contains(&line) {
                let center = pos2(left + MARKER_WIDTH / 2.0 + 2.0, top + row_height / 2.0);
                painter.circle_filled(
                    center,
                    MARKER_WIDTH / 2.0 - 2.0,
                    visuals.selection.stroke.color,
                );
            }
            let color = if line == cursor_line {
                palette.text
            } else {
                palette.line_number
            };
            painter.text(
                pos2(numbers_right - GUTTER_PADDING, top),
                Align2::RIGHT_TOP,
                (line + 1).to_string(),
                self.layouter.font.clone(),
                color,
            );
            if self.folding {
                let center = pos2(gutter.right() - FOLD_WIDTH / 2.0, top + row_height / 2.0);
                let header = hidden == 0
                    && line_len(rope, line) <= LONG_LINE
                    && fold::is_header(rope, line, self.layouter.tab_width);
                let points = if hidden > 0 {
                    vec![
                        center + vec2(-2.0, -4.0),
                        center + vec2(3.0, 0.0),
                        center + vec2(-2.0, 4.0),
                    ]
                } else if header {
                    vec![
                        center + vec2(-4.0, -2.0),
                        center + vec2(4.0, -2.0),
                        center + vec2(0.0, 3.0),
                    ]
                } else {
                    continue;
                };
                let color = if hidden > 0 {
                    palette.text
                } else {
                    palette.faint()
                };
                painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
            }
        }
    }
}

/// A small "continues below" hook at the end of a wrapped row.
fn paint_wrap_symbol(ui: &Ui, at: Pos2, color: Color32) {
    let stroke = Stroke::new(1.0, color);
    let painter = ui.painter();
    let (right, bottom) = (at + vec2(6.0, -3.0), at + vec2(6.0, 3.0));
    painter.line_segment([right, bottom], stroke);
    painter.line_segment([bottom, at + vec2(1.0, 3.0)], stroke);
    painter.line_segment([at + vec2(1.0, 3.0), at + vec2(3.0, 1.0)], stroke);
    painter.line_segment([at + vec2(1.0, 3.0), at + vec2(3.0, 5.0)], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_line_ignores_newline() {
        assert_eq!(max_line_chars(&Rope::from_str("ab\nabcd\n")), 4);
        assert_eq!(max_line_chars(&Rope::new()), 0);
    }

    #[test]
    fn keeps_column_on_its_row() {
        assert_eq!(clamp_to_row(7, &(4..7), false), 6);
        assert_eq!(clamp_to_row(7, &(4..7), true), 7);
        assert_eq!(clamp_to_row(3, &(3..3), false), 3);
    }

    #[test]
    fn recognizes_recordable_input() {
        assert!(is_recordable(&Event::Text("a".into())));
        assert!(!is_recordable(&Event::PointerGone));
    }

    #[test]
    fn updates_wrapped_rows_incrementally() {
        let layouter = Layouter {
            ctx: egui::Context::default(),
            font: FontId::monospace(12.0),
            palette: Palette::from_visuals(&egui::Visuals::light()),
            tab_width: 4,
            char_width: 7.0,
            wrap_width: Some(10),
            visible_columns: 0..80,
            long_segments: RefCell::default(),
            segment_revision: Cell::new(0),
        };
        let mut doc = Document::new();
        let long = "word ".repeat(LONG_LINE / 4);
        doc.replace(
            0..0,
            &format!("short\n{}\nx\n{long}\nend", "a b c ".repeat(5)),
        );
        let mut state = EditorState::default();
        let edits: [(Range<usize>, &str); 6] = [
            (0..0, "a much longer first line\n"),
            (3..3, "\n\n"),
            (10..40, ""),
            (50..50, "\tsome tabs\tand words that wrap\n"),
            (0..5, "zz"),
            (200..300, "\n"),
        ];
        layouter.row_map(&doc, &mut state, &[]);
        for (range, text) in edits {
            let len = doc.text().len_chars();
            doc.replace(range.start.min(len)..range.end.min(len), text);
            layouter.row_map(&doc, &mut state, &[]);
            let incremental = state.wrap_cache.as_ref().unwrap().extra_rows.clone();
            let full = layouter.extra_rows_in(doc.text(), 0..doc.text().len_lines());
            assert_eq!(incremental, full, "after replacing {range:?} with {text:?}");
        }
    }
}
