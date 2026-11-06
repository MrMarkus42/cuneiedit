//! Side-by-side comparison of two tabs, or of a tab and its saved file.

use std::time::Duration;

use eframe::egui::{self, Rect, Ui, UiBuilder, pos2};
use pilcrow_core::Document;
use pilcrow_core::highlight::Highlighter;
use pilcrow_editor::{Editor, EditorState};

use super::PilcrowApp;
use super::command::Command;
use super::editor_view::styled;
use super::tab::Tab;
use crate::compare::{CompareState, CompareView, Decorations, SavedCopy, Side};
use crate::i18n::{Texts, fill};
use crate::settings::Settings;

impl PilcrowApp {
    /// Compares the tab set as first (or the neighboring tab) with the active tab.
    pub(super) fn compare_with_neighbor(&mut self) {
        let right = self.tabs[self.active].id;
        let left = self
            .compare_first
            .take()
            .filter(|&id| id != right && self.tabs.iter().any(|tab| tab.id == id))
            .or_else(|| {
                let neighbor = if self.active > 0 { self.active - 1 } else { 1 };
                self.tabs.get(neighbor).map(|tab| tab.id)
            });
        if let Some(left) = left {
            self.compare = Some(CompareView::new(Side::Tab(left), Side::Tab(right)));
        }
    }

    pub(super) fn compare_with_file(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        let left = self.tabs[self.active].id;
        self.open_path(&path);
        let right = self.tabs[self.active].id;
        if right != left {
            self.compare = Some(CompareView::new(Side::Tab(left), Side::Tab(right)));
        }
    }

    pub(super) fn compare_with_saved(&mut self) {
        let texts = self.texts();
        let tab = &self.tabs[self.active];
        let Some(path) = tab.doc.path() else {
            return;
        };
        match Document::open(path) {
            Ok(doc) => {
                let saved = SavedCopy {
                    doc,
                    editor: EditorState::default(),
                    highlighter: Highlighter::new(tab.syntax()),
                    title: fill(texts.saved_version, &[("name", &tab.title(texts))]),
                };
                let right = Side::Tab(tab.id);
                self.compare = Some(CompareView::new(Side::Saved(Box::new(saved)), right));
            }
            Err(err) => {
                let name = path.display().to_string();
                let message = fill(
                    texts.open_failed,
                    &[("name", &name), ("error", &err.to_string())],
                );
                self.error(message);
            }
        }
    }

    pub(super) fn go_to_difference(&mut self, forward: bool) {
        let texts = self.settings.language.texts();
        if let Some((left, right, state)) = compare_panes(&mut self.compare, &mut self.tabs, texts)
        {
            state.go_to_difference(forward, (left.doc, left.editor), (right.doc, right.editor));
        }
    }

    pub(super) fn compare_bar(&self, ui: &mut Ui) -> Option<Command> {
        let texts = self.texts();
        let state = &self.compare.as_ref()?.state;
        let mut command = None;
        ui.horizontal(|ui| {
            if ui
                .button("↑")
                .on_hover_text(texts.previous_difference)
                .clicked()
            {
                command = Some(Command::PreviousDifference);
            }
            if ui
                .button("↓")
                .on_hover_text(texts.next_difference)
                .clicked()
            {
                command = Some(Command::NextDifference);
            }
            let (changed, added, removed) = state.counts();
            if state.result().is_identical() {
                ui.label(texts.identical);
            } else {
                ui.label(fill(
                    texts.differences,
                    &[
                        ("changed", &changed.to_string()),
                        ("added", &added.to_string()),
                        ("removed", &removed.to_string()),
                    ],
                ));
            }
            ui.separator();
            let mut whitespace = state.options.ignore_whitespace;
            if ui
                .checkbox(&mut whitespace, texts.ignore_whitespace)
                .clicked()
            {
                command = Some(Command::ToggleIgnoreWhitespace);
            }
            let mut case = state.options.ignore_case;
            if ui.checkbox(&mut case, texts.ignore_case).clicked() {
                command = Some(Command::ToggleIgnoreCase);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(texts.end_compare).clicked() {
                    command = Some(Command::EndCompare);
                }
            });
        });
        command
    }

    pub(super) fn show_compare(&mut self, ui: &mut Ui) {
        let texts = self.settings.language.texts();
        let Some((mut left, mut right, state)) =
            compare_panes(&mut self.compare, &mut self.tabs, texts)
        else {
            // One of the compared tabs was closed.
            self.compare = None;
            return;
        };
        let now = ui.input(|i| i.time);
        if let Some(retry) = state.update(left.doc, right.doc, ui.visuals().dark_mode, now) {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(retry));
        }

        let rect = ui.max_rect();
        let middle = rect.center().x;
        let left_rect = Rect::from_min_max(rect.min, pos2(middle - 1.0, rect.max.y));
        let right_rect = Rect::from_min_max(pos2(middle + 1.0, rect.min.y), rect.max);
        let (left_decorations, right_decorations) = state.decorations();
        let settings = &self.settings;
        let left_focus = show_pane(ui, left_rect, &mut left, left_decorations, settings, "left");
        let right_focus = show_pane(
            ui,
            right_rect,
            &mut right,
            right_decorations,
            settings,
            "right",
        );
        ui.painter().vline(
            middle,
            rect.y_range(),
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        if state.sync_scroll(left.editor, right.editor) {
            ui.ctx().request_repaint();
        }

        let focused = match (left_focus, right_focus) {
            (true, _) => left.id,
            (_, true) => right.id,
            _ => None,
        };
        if let Some(index) = focused.and_then(|id| self.tabs.iter().position(|tab| tab.id == id)) {
            self.active = index;
        }
    }
}

/// One side of a comparison.
struct Pane<'a> {
    /// Tab id; `None` for the read-only saved copy.
    id: Option<u64>,
    title: String,
    read_only: bool,
    doc: &'a mut Document,
    editor: &'a mut EditorState,
    highlighter: &'a mut Highlighter,
}

impl<'a> Pane<'a> {
    fn tab(tab: &'a mut Tab, texts: &Texts) -> Self {
        let title = tab.title(texts);
        Self {
            id: Some(tab.id),
            title,
            read_only: tab.read_only,
            doc: &mut tab.doc,
            editor: &mut tab.editor,
            highlighter: &mut tab.highlighter,
        }
    }

    fn saved(saved: &'a mut SavedCopy) -> Self {
        Self {
            id: None,
            title: saved.title.clone(),
            read_only: true,
            doc: &mut saved.doc,
            editor: &mut saved.editor,
            highlighter: &mut saved.highlighter,
        }
    }
}

/// Both sides of the active comparison, or `None` if a compared tab no longer exists.
fn compare_panes<'a>(
    compare: &'a mut Option<CompareView>,
    tabs: &'a mut [Tab],
    texts: &Texts,
) -> Option<(Pane<'a>, Pane<'a>, &'a mut CompareState)> {
    let view = compare.as_mut()?;
    let index = |side: &Side| match side {
        Side::Tab(id) => tabs.iter().position(|tab| tab.id == *id).map(Some),
        Side::Saved(_) => Some(None),
    };
    let (left_index, right_index) = (index(&view.left)?, index(&view.right)?);
    let CompareView { left, right, state } = view;
    let panes = match (left_index, right_index, left, right) {
        (Some(a), Some(b), _, _) if a != b => {
            let (first, second) = pair_mut(tabs, a, b);
            (Pane::tab(first, texts), Pane::tab(second, texts))
        }
        (None, Some(b), Side::Saved(saved), _) => {
            (Pane::saved(saved), Pane::tab(&mut tabs[b], texts))
        }
        (Some(a), None, _, Side::Saved(saved)) => {
            (Pane::tab(&mut tabs[a], texts), Pane::saved(saved))
        }
        _ => return None,
    };
    Some((panes.0, panes.1, state))
}

fn pair_mut<T>(items: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    if a < b {
        let (first, second) = items.split_at_mut(b);
        (&mut first[a], &mut second[0])
    } else {
        let (first, second) = items.split_at_mut(a);
        (&mut second[0], &mut first[b])
    }
}

/// Shows one side of a comparison and returns whether its editor has focus.
fn show_pane(
    ui: &mut Ui,
    rect: Rect,
    pane: &mut Pane<'_>,
    decorations: &Decorations,
    settings: &Settings,
    salt: &str,
) -> bool {
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let marker = if pane.doc.is_modified() { " ●" } else { "" };
            ui.strong(format!("{}{marker}", pane.title));
        });
        let editor = Editor::new(("pilcrow-compare", salt), &mut *pane.doc, &mut *pane.editor);
        styled(editor, settings)
            .highlighter(&mut *pane.highlighter)
            .read_only(pane.read_only)
            .line_backgrounds(&decorations.backgrounds)
            .spans(&decorations.spans)
            .spacers(&decorations.spacers)
            .show(ui)
            .has_focus()
    })
    .inner
}
