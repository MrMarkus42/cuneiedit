//! Panels for moving around: the folder tree and the document map.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cuneiedit_core::files::{DirEntry, list_dir};
use cuneiedit_core::highlight::TokenKind;
use cuneiedit_editor::Palette;
use eframe::egui::{CollapsingHeader, Panel, Rect, ScrollArea, Sense, Ui};

use super::CuneiEditApp;
use super::command::Command;

/// Height of a line in the document map.
const MAP_ROW: f32 = 2.0;
/// Columns shown per line in the document map.
const MAP_COLUMNS: usize = 120;
/// Longer lines are drawn without syntax colors, which would need lexing them in full.
const MAP_MAX_LEXED: usize = 2_000;

/// Folder listings of the file tree, read when a folder is first shown.
#[derive(Default)]
pub struct FileTree {
    listings: HashMap<PathBuf, Vec<DirEntry>>,
}

impl FileTree {
    fn entries(&mut self, dir: &Path) -> Vec<DirEntry> {
        self.listings
            .entry(dir.to_path_buf())
            .or_insert_with(|| list_dir(dir, false).unwrap_or_default())
            .clone()
    }

    pub fn refresh(&mut self) {
        self.listings.clear();
    }
}

fn show_dir(ui: &mut Ui, tree: &mut FileTree, dir: &Path, open: &mut Option<PathBuf>) {
    for entry in tree.entries(dir) {
        if entry.is_dir {
            CollapsingHeader::new(format!("📁 {}", entry.name))
                .id_salt(&entry.path)
                .show(ui, |ui| show_dir(ui, tree, &entry.path, open));
        } else if ui
            .selectable_label(false, &entry.name)
            .on_hover_text(entry.path.display().to_string())
            .clicked()
        {
            *open = Some(entry.path.clone());
        }
    }
}

fn token_color(palette: &Palette, kind: Option<TokenKind>) -> eframe::egui::Color32 {
    palette.color(kind)
}

impl CuneiEditApp {
    pub(super) fn file_tree_panel(&mut self, ui: &mut Ui, commands: &mut Vec<Command>) {
        let texts = self.texts();
        Panel::left("cuneiedit-file-tree")
            .resizable(true)
            .default_size(200.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(texts.file_tree);
                    if ui.small_button("⟳").on_hover_text(texts.refresh).clicked() {
                        self.tree.refresh();
                    }
                    if ui
                        .small_button("…")
                        .on_hover_text(texts.open_folder)
                        .clicked()
                    {
                        commands.push(Command::OpenFolder);
                    }
                });
                ui.separator();
                let Some(root) = self.settings.workspace.clone() else {
                    ui.weak(texts.no_folder);
                    return;
                };
                let mut open = None;
                ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                    let name = root.file_name().map_or_else(
                        || root.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    CollapsingHeader::new(format!("📁 {name}"))
                        .id_salt(&root)
                        .default_open(true)
                        .show(ui, |ui| show_dir(ui, &mut self.tree, &root, &mut open));
                });
                if let Some(path) = open {
                    self.open_path(&path);
                }
            });
    }

    /// A miniature of the text around the visible part; a click or drag scrolls there.
    pub(super) fn document_map_panel(&mut self, ui: &mut Ui) {
        let palette = self.palette();
        let tab_width = self.settings.tab_width;
        Panel::right("cuneiedit-map")
            .resizable(true)
            .default_size(140.0)
            .show(ui, |ui| {
                let palette = palette.unwrap_or_else(|| Palette::from_visuals(ui.visuals()));
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
                let tab = &mut self.tabs[self.active];
                let rope = tab.doc.text();
                let total = rope.len_lines();
                let visible = tab.editor.visible_lines();
                let rows = ((rect.height() / MAP_ROW) as usize).max(1);
                let center = (visible.start + visible.end) / 2;
                let first = center
                    .saturating_sub(rows / 2)
                    .min(total.saturating_sub(rows));
                let last = (first + rows).min(total);
                let scale = (rect.width() / MAP_COLUMNS as f32).clamp(0.5, 1.5);
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, palette.background);

                for line in first..last {
                    let top = rect.top() + (line - first) as f32 * MAP_ROW;
                    let short = rope.line(line).len_chars() <= MAP_MAX_LEXED;
                    let tokens = if short {
                        tab.highlighter.tokens(&tab.doc, line)
                    } else {
                        Vec::new()
                    };
                    let kind_at = |col: usize| {
                        tokens
                            .iter()
                            .find(|t| t.start <= col && col < t.end)
                            .map(|t| t.kind)
                    };
                    let mut display = 0;
                    let mut run: Option<(usize, usize, Option<TokenKind>)> = None;
                    let flush = |run: &mut Option<(usize, usize, Option<TokenKind>)>| {
                        if let Some((start, end, kind)) = run.take() {
                            let x = rect.left() + start as f32 * scale;
                            let block = Rect::from_x_y_ranges(
                                x..=x + (end - start) as f32 * scale,
                                top..=top + MAP_ROW * 0.8,
                            );
                            let color = token_color(&palette, kind).gamma_multiply(0.7);
                            painter.rect_filled(block, 0.0, color);
                        }
                    };
                    for (col, c) in rope.line(line).chars().enumerate() {
                        if display >= MAP_COLUMNS || c == '\n' {
                            break;
                        }
                        let width = if c == '\t' {
                            tab_width - display % tab_width.max(1)
                        } else {
                            1
                        };
                        if c.is_whitespace() {
                            flush(&mut run);
                        } else {
                            let kind = kind_at(col);
                            match &mut run {
                                Some((_, end, k)) if *k == kind => *end = display + width,
                                _ => {
                                    flush(&mut run);
                                    run = Some((display, display + width, kind));
                                }
                            }
                        }
                        display += width;
                    }
                    flush(&mut run);
                }

                let row_y = |line: usize| {
                    rect.top() + line.clamp(first, last) as f32 * MAP_ROW - first as f32 * MAP_ROW
                };
                let shown = Rect::from_x_y_ranges(
                    rect.x_range(),
                    row_y(visible.start)..=row_y(visible.end).max(row_y(visible.start) + 2.0),
                );
                painter.rect_filled(shown, 0.0, palette.selection.gamma_multiply(0.4));

                if response.dragged() {
                    let delta = (response.drag_delta().y / MAP_ROW).round() as isize;
                    if delta != 0 {
                        let target = center.saturating_add_signed(delta).min(total - 1);
                        tab.editor.scroll_to_line(target);
                    }
                } else if response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let line = first + ((pos.y - rect.top()) / MAP_ROW) as usize;
                    tab.editor.scroll_to_line(line.min(total - 1));
                }
            });
    }
}
