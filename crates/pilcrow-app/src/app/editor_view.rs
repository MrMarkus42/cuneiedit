//! The editor area: one or two views of the active document, a second document next to it,
//! word and path completion, and call tips.

use std::ops::Range;
use std::path::Path;

use eframe::egui::{
    self, Align2, Color32, Event, FontId, Frame, Id, Key, Modifiers, Order, Rect, Response,
    ScrollArea, Ui, UiBuilder, pos2,
};
use pilcrow_core::Document;
use pilcrow_core::code;
use pilcrow_core::files::complete_path;
use pilcrow_core::highlight::Highlighter;
use pilcrow_core::ops::CommentMode;
use pilcrow_core::transform::Case;
use pilcrow_editor::{Editor, EditorState};

use super::PilcrowApp;
use super::command::{Command, LineOp};
use super::tab::Tab;
use crate::settings::{Flag, Settings};
use crate::themes;

/// Call tips look up definitions only in documents up to this size.
const MAX_CALL_TIP_LINES: usize = 100_000;
/// Text width in distraction free mode, in characters.
const DISTRACTION_FREE_COLUMNS: f32 = 100.0;

/// Open completion popup.
pub struct Completion {
    pub tab: u64,
    /// Start of the word (or last path part) being completed.
    start: usize,
    items: Vec<String>,
    selected: usize,
    scroll: bool,
    /// Completing a file path instead of a word.
    path: bool,
}

/// The definition of the function whose arguments are being typed.
pub struct CallTip {
    pub tab: u64,
    /// Position of the opening parenthesis.
    open: usize,
    text: String,
}

/// An editor with the user's view settings applied.
pub fn styled<'a>(editor: Editor<'a>, settings: &Settings) -> Editor<'a> {
    editor
        .font(FontId::monospace(settings.font_size))
        .tab_width(settings.tab_width)
        .indent_with_spaces(settings.get(Flag::IndentWithSpaces))
        .show_whitespace(settings.get(Flag::ShowWhitespace))
        .show_line_endings(settings.get(Flag::ShowLineEndings))
        .show_wrap_symbol(settings.get(Flag::ShowWrapSymbol))
        .indent_guides(settings.get(Flag::IndentGuides))
        .word_wrap(settings.get(Flag::WordWrap))
        .auto_close(settings.get(Flag::AutoClose))
        .edge_column(settings.get(Flag::EdgeLine).then_some(settings.edge_column))
        .smart_highlight(settings.get(Flag::SmartHighlight))
        .palette(themes::palette(&settings.color_theme))
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Characters that end a file path being typed.
fn ends_path(c: char) -> bool {
    c.is_whitespace() || "\"'`<>()[]{},;=|".contains(c)
}

type Marks<'a> = &'a [(Range<usize>, Color32)];

/// What an editor view shows besides the text; shared by both views of a split.
struct Look<'a> {
    settings: &'a Settings,
    id: u64,
    highlights: &'a [Range<usize>],
    spans: Marks<'a>,
    gutter: Marks<'a>,
    read_only: bool,
    markup: bool,
}

impl<'a> Look<'a> {
    fn new(
        settings: &'a Settings,
        tab: &'a mut Tab,
        highlights: &'a [Range<usize>],
    ) -> (
        Self,
        &'a mut Document,
        &'a mut EditorState,
        &'a mut Highlighter,
    ) {
        let Tab {
            id,
            doc,
            editor,
            highlighter,
            read_only,
            marks,
            history,
            ..
        } = tab;
        let gutter: &[_] = if settings.get(Flag::ChangeHistory) {
            history.marks(doc)
        } else {
            &[]
        };
        let look = Self {
            settings,
            id: *id,
            highlights,
            spans: marks.spans(doc),
            gutter,
            read_only: *read_only,
            markup: matches!(highlighter.language().name(), "HTML" | "XML"),
        };
        (look, doc, editor, highlighter)
    }

    fn show(
        &self,
        ui: &mut Ui,
        salt: &'static str,
        doc: &mut Document,
        state: &mut EditorState,
        highlighter: &mut Highlighter,
    ) -> Response {
        let editor = Editor::new(("pilcrow-editor", self.id, salt), doc, state);
        styled(editor, self.settings)
            .highlighter(highlighter)
            .highlights(self.highlights)
            .spans(self.spans)
            .gutter_marks(self.gutter)
            .read_only(self.read_only)
            .folding(self.settings.get(Flag::Folding))
            .match_tags(self.markup)
            .show(ui)
    }
}

/// Two tabs of `tabs`, mutably.
fn two_tabs(tabs: &mut [Tab], a: usize, b: usize) -> (&mut Tab, &mut Tab) {
    if a < b {
        let (left, right) = tabs.split_at_mut(b);
        (&mut left[a], &mut right[0])
    } else {
        let (left, right) = tabs.split_at_mut(a);
        (&mut right[0], &mut left[b])
    }
}

/// Left and right half of `rect`, with a gap for the divider.
fn halves(rect: Rect) -> (Rect, Rect) {
    let middle = rect.center().x;
    (
        Rect::from_min_max(rect.min, pos2(middle - 1.0, rect.max.y)),
        Rect::from_min_max(pos2(middle + 1.0, rect.min.y), rect.max),
    )
}

impl PilcrowApp {
    pub(super) fn show_editor(&mut self, ui: &mut Ui, commands: &mut Vec<Command>) {
        let texts = self.settings.language.texts();
        let read_only = self.tabs[self.active].read_only;
        let revision = self.tabs[self.active].doc.revision();
        let active_before = self.active;
        let pair = self.view_pair();
        let response = if self.distraction_free {
            let rect = ui.max_rect();
            let width = (self.settings.font_size * 0.62 * DISTRACTION_FREE_COLUMNS + 80.0)
                .min(rect.width());
            let column = Rect::from_center_size(rect.center(), egui::vec2(width, rect.height()));
            ui.scope_builder(UiBuilder::new().max_rect(column), |ui| {
                self.show_views(ui, None)
            })
            .inner
        } else {
            self.show_views(ui, pair)
        };
        let tab = &mut self.tabs[self.active];
        if self.active == active_before && tab.monitoring && tab.doc.revision() != revision {
            tab.editor.set_cursor(tab.doc.text().len_chars());
        }
        self.editor_focused = response.has_focus();
        if response.changed() && self.find.open {
            // Highlights were computed before this edit.
            ui.ctx().request_repaint();
        }
        let tab = &self.tabs[self.active];
        response.context_menu(|ui| {
            let has_selection = !tab.editor.selection().is_empty();
            let items = [
                (Command::Undo, tab.doc.can_undo() && !read_only),
                (Command::Redo, tab.doc.can_redo() && !read_only),
                (Command::Cut, has_selection && !read_only),
                (Command::Copy, has_selection),
                (Command::Paste, !read_only),
                (Command::SelectAll, true),
                (
                    Command::Line(LineOp::Comment(CommentMode::Toggle)),
                    tab.syntax().line_comment().is_some() && !read_only,
                ),
                (
                    Command::Line(LineOp::Case(Case::Upper)),
                    has_selection && !read_only,
                ),
                (
                    Command::Line(LineOp::Case(Case::Lower)),
                    has_selection && !read_only,
                ),
            ];
            for (index, (command, enabled)) in items.into_iter().enumerate() {
                if matches!(index, 2 | 5 | 6) {
                    ui.separator();
                }
                let shortcut = self.keymap.get(command);
                if super::menu::menu_item(ui, &command.label(texts), shortcut, enabled) {
                    commands.push(command);
                }
            }
            ui.separator();
            ui.menu_button(texts.mark_menu, |ui| {
                for style in 0..super::tab::MARK_STYLES {
                    let command = Command::Mark(style);
                    if super::menu::menu_item(ui, &command.label(texts), None, true) {
                        commands.push(command);
                    }
                }
            });
        });

        if response.changed() {
            let typed: Vec<String> = ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        Event::Text(text) => Some(text.clone()),
                        _ => None,
                    })
                    .collect()
            });
            if typed.iter().any(|t| t.ends_with('(')) {
                self.start_call_tip();
            }
            if self.completion.is_some() {
                let path = self.completion.as_ref().is_some_and(|c| c.path);
                self.start_completion(1, path);
            } else if !typed.is_empty() && self.settings.get(Flag::AutoComplete) {
                self.start_completion(2, false);
            }
        }
        self.check_completion();
        self.check_call_tip();
    }

    /// Indices of the tabs shown left and right when a tab was moved to the other view. The
    /// active tab is on its side; the other side shows the tab last active there.
    fn view_pair(&self) -> Option<(usize, usize)> {
        let right = self.second_view.and_then(|id| self.tab_index(id))?;
        let left = if right == self.active {
            self.left_view
                .and_then(|id| self.tab_index(id))
                .filter(|&index| index != right)?
        } else {
            self.active
        };
        Some((left, right))
    }

    /// The active tab, with another tab or a second view of itself next to it. Returns the
    /// response of the focused view; focusing the other one makes it the active one.
    fn show_views(&mut self, ui: &mut Ui, pair: Option<(usize, usize)>) -> Response {
        let settings = &self.settings;
        let id = self.tabs[self.active].id;
        if let Some((left_index, right_index)) = pair {
            let active_is_right = right_index == self.active;
            let (left_tab, right_tab) = two_tabs(&mut self.tabs, left_index, right_index);
            self.left_view = Some(left_tab.id);
            let active_doc = if active_is_right {
                &right_tab.doc
            } else {
                &left_tab.doc
            };
            let highlights: &[Range<usize>] = if self.find.open {
                self.find.matches(id, active_doc)
            } else {
                &[]
            };
            let (left_highlights, right_highlights) = if active_is_right {
                (&[][..], highlights)
            } else {
                (highlights, &[][..])
            };
            let (left_rect, right_rect) = halves(ui.max_rect());
            let (look, doc, state, highlighter) = Look::new(settings, left_tab, left_highlights);
            let left = ui
                .scope_builder(UiBuilder::new().max_rect(left_rect), |ui| {
                    look.show(ui, "left-view", doc, state, highlighter)
                })
                .inner;
            let (look, doc, state, highlighter) = Look::new(settings, right_tab, right_highlights);
            let right = ui
                .scope_builder(UiBuilder::new().max_rect(right_rect), |ui| {
                    look.show(ui, "right-view", doc, state, highlighter)
                })
                .inner;
            let rect = ui.max_rect();
            ui.painter().vline(
                rect.center().x,
                rect.y_range(),
                ui.visuals().widgets.noninteractive.bg_stroke,
            );
            let right_wanted = right.has_focus() || right.clicked();
            let left_wanted = left.has_focus() || left.clicked();
            if right_wanted && !active_is_right {
                self.active = right_index;
                return right;
            }
            if left_wanted && active_is_right {
                self.active = left_index;
                return left;
            }
            return if active_is_right { right } else { left };
        }

        let tab = &mut self.tabs[self.active];
        let highlights: &[Range<usize>] = if self.find.open {
            self.find.matches(id, &tab.doc)
        } else {
            &[]
        };
        let Some(mut split) = tab.split.take() else {
            let (look, doc, state, highlighter) = Look::new(settings, tab, highlights);
            return look.show(ui, "main", doc, state, highlighter);
        };
        // Both views share the bookmarks of the focused one.
        tab.editor.track_bookmarks(&tab.doc);
        let marks = tab.editor.bookmarks().clone();
        split.other.set_bookmarks(&tab.doc, marks);

        let (left_rect, right_rect) = halves(ui.max_rect());
        let main_on_left = split.main_on_left;
        let (look, doc, state, highlighter) = Look::new(settings, tab, highlights);
        let (left, right) = if main_on_left {
            (state, &mut split.other)
        } else {
            (&mut split.other, state)
        };
        let left = ui
            .scope_builder(UiBuilder::new().max_rect(left_rect), |ui| {
                look.show(ui, "left", doc, left, highlighter)
            })
            .inner;
        let right = ui
            .scope_builder(UiBuilder::new().max_rect(right_rect), |ui| {
                look.show(ui, "right", doc, right, highlighter)
            })
            .inner;
        let rect = ui.max_rect();
        ui.painter().vline(
            rect.center().x,
            rect.y_range(),
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        let (main, other) = if main_on_left {
            (left, right)
        } else {
            (right, left)
        };
        let tab = &mut self.tabs[self.active];
        let response = if other.has_focus() || other.clicked() {
            std::mem::swap(&mut tab.editor, &mut split.other);
            split.main_on_left ^= true;
            other
        } else {
            main
        };
        tab.split = Some(split);
        response
    }

    /// Shows the definition of the function before a just typed `(`.
    fn start_call_tip(&mut self) {
        self.call_tip = None;
        let tab = &self.tabs[self.active];
        let rope = tab.doc.text();
        if rope.len_lines() > MAX_CALL_TIP_LINES || tab.editor.has_multiple_selections() {
            return;
        }
        let cursor = tab.editor.cursor().min(rope.len_chars());
        let Some(open) = cursor.checked_sub(1).filter(|&p| rope.char(p) == '(') else {
            return;
        };
        let mut start = open;
        while start > 0 && is_word(rope.char(start - 1)) {
            start -= 1;
        }
        let name = tab.doc.slice(start..open);
        if name.is_empty() {
            return;
        }
        if let Some(text) = code::signature(&tab.doc, tab.syntax(), &name) {
            self.call_tip = Some(CallTip {
                tab: tab.id,
                open,
                text,
            });
        }
    }

    /// Closes the call tip once the cursor leaves the parentheses.
    fn check_call_tip(&mut self) {
        let Some(tip) = &self.call_tip else {
            return;
        };
        let tab = &self.tabs[self.active];
        let rope = tab.doc.text();
        let cursor = tab.editor.cursor();
        let inside = tip.tab == tab.id
            && tip.open < cursor
            && cursor <= rope.len_chars()
            && rope.char(tip.open) == '('
            && rope.char_to_line(tip.open) == rope.char_to_line(cursor)
            && {
                let mut depth = 0i32;
                rope.slice(tip.open + 1..cursor).chars().all(|c| {
                    depth += i32::from(c == '(') - i32::from(c == ')');
                    depth >= 0
                })
            };
        if !inside {
            self.call_tip = None;
        }
    }

    pub(super) fn call_tip_popup(&mut self, ctx: &egui::Context) {
        let Some(tip) = &self.call_tip else {
            return;
        };
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.call_tip = None;
            return;
        }
        let Some(cursor) = self.tabs[self.active].editor.cursor_screen_rect() else {
            return;
        };
        let font = FontId::monospace(self.settings.font_size * 0.9);
        egui::Area::new(Id::unique("pilcrow-call-tip"))
            .order(Order::Tooltip)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(cursor.left_top())
            .interactable(false)
            .show(ctx, |ui| {
                Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(&tip.text).font(font));
                });
            });
    }

    /// Opens the completion popup for the word before the cursor, if it has at least
    /// `min_len` chars and there are suggestions. With `paths` (or a typed path containing
    /// `/`, asked for explicitly), the entries of the folder being typed are offered.
    pub(super) fn start_completion(&mut self, min_len: usize, paths: bool) {
        self.completion = None;
        let tab = &self.tabs[self.active];
        if tab.read_only || !tab.editor.selection().is_empty() {
            return;
        }
        let rope = tab.doc.text();
        let cursor = tab.editor.cursor().min(rope.len_chars());
        if paths || min_len == 1 {
            let mut path_start = cursor;
            while path_start > 0 && !ends_path(rope.char(path_start - 1)) {
                path_start -= 1;
            }
            let typed = tab.doc.slice(path_start..cursor);
            if typed.contains('/') {
                let base = tab
                    .doc
                    .path()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_default();
                let items = complete_path(&base, &typed, 50);
                let last_part = typed.rsplit('/').next().unwrap_or("").chars().count();
                if !items.is_empty() {
                    self.completion = Some(Completion {
                        tab: tab.id,
                        start: cursor - last_part,
                        items,
                        selected: 0,
                        scroll: false,
                        path: true,
                    });
                }
                return;
            }
        }
        let mut start = cursor;
        while start > 0 && is_word(rope.char(start - 1)) {
            start -= 1;
        }
        let prefix = tab.doc.slice(start..cursor);
        if prefix.chars().count() < min_len {
            return;
        }
        let items: Vec<String> = code::completions(&tab.doc, tab.syntax(), &prefix, 50)
            .into_iter()
            .filter(|item| *item != prefix)
            .collect();
        if !items.is_empty() {
            self.completion = Some(Completion {
                tab: tab.id,
                start,
                items,
                selected: 0,
                scroll: false,
                path: false,
            });
        }
    }

    /// Closes the popup if the cursor left the word or another tab is active.
    fn check_completion(&mut self) {
        let Some(completion) = &self.completion else {
            return;
        };
        let tab = &self.tabs[self.active];
        let cursor = tab.editor.cursor();
        let rope = tab.doc.text();
        let part_char = |c: char| {
            if completion.path {
                !ends_path(c) && c != '/'
            } else {
                is_word(c)
            }
        };
        let valid = completion.tab == tab.id
            && completion.start <= cursor
            && cursor <= rope.len_chars()
            && rope.slice(completion.start..cursor).chars().all(part_char)
            && tab.editor.selection().is_empty();
        if !valid {
            self.completion = None;
        }
    }

    /// Handles keys for the popup before the editor sees them.
    pub(super) fn completion_keys(&mut self, ctx: &egui::Context) {
        if self.completion.is_none() || !self.editor_focused || self.dialog.is_some() {
            return;
        }
        let (up, down, accept, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter)
                    || i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.completion = None;
        } else if accept {
            self.accept_completion(None);
        } else if let Some(completion) = &mut self.completion {
            let len = completion.items.len();
            if down {
                completion.selected = (completion.selected + 1) % len;
                completion.scroll = true;
            } else if up {
                completion.selected = (completion.selected + len - 1) % len;
                completion.scroll = true;
            }
        }
    }

    fn accept_completion(&mut self, index: Option<usize>) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        let Some(item) = completion.items.get(index.unwrap_or(completion.selected)) else {
            return;
        };
        let tab = self.tab();
        let cursor = tab.editor.cursor();
        tab.doc.seal_undo_group();
        let end = tab.doc.replace(completion.start..cursor, item);
        tab.doc.seal_undo_group();
        tab.editor.set_cursor(end);
        tab.editor.request_focus();
    }

    pub(super) fn completion_popup(&mut self, ctx: &egui::Context) {
        let Some(completion) = &mut self.completion else {
            return;
        };
        let Some(cursor) = self.tabs[self.active].editor.cursor_screen_rect() else {
            return;
        };
        let mut clicked = None;
        egui::Area::new(Id::unique("pilcrow-completion"))
            .order(Order::Foreground)
            .fixed_pos(cursor.left_bottom())
            .show(ctx, |ui| {
                Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(160.0);
                    ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        for (index, item) in completion.items.iter().enumerate() {
                            let label = ui.selectable_label(index == completion.selected, item);
                            if index == completion.selected && completion.scroll {
                                label.scroll_to_me(None);
                            }
                            if label.clicked() {
                                clicked = Some(index);
                            }
                        }
                    });
                });
            });
        completion.scroll = false;
        if clicked.is_some() {
            self.accept_completion(clicked);
        }
    }
}
