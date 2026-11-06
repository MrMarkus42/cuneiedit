//! Side and bottom panels: search results, function list, document list, clipboard
//! history and character panel.

use std::path::PathBuf;

use eframe::egui::{self, Panel, RichText, ScrollArea, TextEdit, Ui};
use pilcrow_core::code::{self, LineHits, Symbol};
use pilcrow_core::files::FileHits;
use pilcrow_core::highlight::Language as Syntax;

use super::PilcrowApp;
use super::command::Command;
use super::file_search::FileSearch;
use crate::i18n::{Texts, fill};
use crate::settings::Flag;

pub enum Source {
    Tab(u64),
    File(PathBuf),
}

struct Group {
    source: Source,
    name: String,
    lines: Vec<LineHits>,
}

/// Rows of the results list: a group header or one line of a group.
#[derive(Clone, Copy)]
enum Row {
    Header(usize),
    Line(usize, usize),
}

pub struct Results {
    title: String,
    status: Option<String>,
    groups: Vec<Group>,
    rows: Vec<Row>,
    hits: usize,
    /// Only lines containing this text are shown.
    filter: String,
    /// Rows passing the filter, for the filter and row count they were computed for.
    visible: Vec<Row>,
    visible_for: Option<(String, usize)>,
    /// Index into `visible` of the result last jumped to.
    current: Option<usize>,
    scroll_to_current: bool,
}

impl Results {
    pub fn new(title: String) -> Self {
        Self {
            title,
            status: None,
            groups: Vec::new(),
            rows: Vec::new(),
            hits: 0,
            filter: String::new(),
            visible: Vec::new(),
            visible_for: None,
            current: None,
            scroll_to_current: false,
        }
    }

    fn refresh_visible(&mut self) {
        let key = (self.filter.clone(), self.rows.len());
        if self.visible_for.as_ref() == Some(&key) {
            return;
        }
        let needle = self.filter.to_lowercase();
        self.visible.clear();
        let mut header = None;
        for &row in &self.rows {
            match row {
                Row::Header(_) => header = Some(row),
                Row::Line(group, line) => {
                    let text = &self.groups[group].lines[line].text;
                    if needle.is_empty() || text.to_lowercase().contains(&needle) {
                        self.visible.extend(header.take());
                        self.visible.push(row);
                    }
                }
            }
        }
        if !needle.is_empty() || self.visible.len() != self.rows.len() {
            self.current = None;
        }
        self.visible_for = Some(key);
    }

    /// The next (or previous) result line, wrapping around.
    fn step(&mut self, forward: bool) -> Option<Target> {
        self.refresh_visible();
        let lines: Vec<usize> = (0..self.visible.len())
            .filter(|&i| matches!(self.visible[i], Row::Line(..)))
            .collect();
        let next = match (self.current, forward) {
            (None, true) => lines.first(),
            (None, false) => lines.last(),
            (Some(current), true) => lines.iter().find(|&&i| i > current).or(lines.first()),
            (Some(current), false) => lines.iter().rev().find(|&&i| i < current).or(lines.last()),
        };
        let &index = next?;
        self.current = Some(index);
        self.scroll_to_current = true;
        self.target(index)
    }

    fn target(&self, index: usize) -> Option<Target> {
        let Row::Line(group, line) = *self.visible.get(index)? else {
            return None;
        };
        let hits = &self.groups[group].lines[line];
        Some(Target {
            source: group,
            line: hits.line,
            hit: hits.hits.first().cloned().unwrap_or(0..0),
        })
    }

    pub fn add(&mut self, source: Source, name: String, lines: Vec<LineHits>) {
        let group = self.groups.len();
        self.rows.push(Row::Header(group));
        self.rows
            .extend((0..lines.len()).map(|line| Row::Line(group, line)));
        self.hits += lines.iter().map(|line| line.hits.len()).sum::<usize>();
        self.groups.push(Group {
            source,
            name,
            lines,
        });
    }

    pub fn add_file(&mut self, hits: FileHits) {
        let name = hits.path.display().to_string();
        self.add(Source::File(hits.path), name, hits.lines);
    }

    pub fn finish_search(&mut self, texts: &Texts) {
        self.status = Some(fill(
            texts.results_summary,
            &[
                ("hits", &self.hits.to_string()),
                ("files", &self.groups.len().to_string()),
            ],
        ));
    }

    pub fn finish(&mut self, search: &FileSearch, texts: &Texts) {
        if search.replacing {
            self.status = Some(fill(
                texts.replaced_in_files,
                &[
                    ("count", &search.replaced.to_string()),
                    ("files", &search.replaced_files.to_string()),
                ],
            ));
        } else {
            self.finish_search(texts);
        }
    }
}

/// Symbols of one document revision.
#[derive(Default)]
pub struct SymbolCache {
    key: Option<(u64, u64, Syntax)>,
    symbols: Vec<Symbol>,
}

/// Where a click in the results panel leads.
struct Target {
    source: usize,
    line: usize,
    hit: std::ops::Range<usize>,
}

impl PilcrowApp {
    pub(super) fn results_panel(&mut self, ui: &mut Ui, commands: &mut Vec<Command>) {
        let texts = self.texts();
        let Some(results) = &mut self.results else {
            return;
        };
        let mut target = None;
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong(format!("{}: {}", texts.results, results.title));
            if let Some(search) = &self.file_search {
                ui.spinner();
                ui.label(fill(
                    texts.searching,
                    &[("files", &search.scanned.to_string())],
                ));
                if ui.button(texts.cancel).clicked() {
                    search.cancel();
                }
            } else if let Some(status) = &results.status {
                ui.label(status);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.small_button("×").clicked();
                ui.add(
                    TextEdit::singleline(&mut results.filter)
                        .hint_text(texts.results_filter)
                        .desired_width(180.0),
                );
            });
        });
        ui.separator();
        results.refresh_visible();
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
        let scroll = std::mem::take(&mut results.scroll_to_current);
        let mut area = ScrollArea::vertical().auto_shrink(false);
        if scroll && let Some(current) = results.current {
            let offset = (current as f32 * row_height - 2.0 * row_height).max(0.0);
            area = area.vertical_scroll_offset(offset);
        }
        area.show_rows(ui, row_height, results.visible.len(), |ui, rows| {
            for index in rows {
                match results.visible[index] {
                    Row::Header(group) => {
                        let group = &results.groups[group];
                        let count: usize = group.lines.iter().map(|l| l.hits.len()).sum();
                        ui.label(RichText::new(format!("{} ({count})", group.name)).strong());
                    }
                    Row::Line(group_index, line) => {
                        let hits = &results.groups[group_index].lines[line];
                        let text = format!("  {:>6}: {}", hits.line + 1, hits.text.trim_end());
                        let selected = results.current == Some(index);
                        let label = ui.selectable_label(selected, RichText::new(text).monospace());
                        if label.clicked() {
                            results.current = Some(index);
                            target = results.target(index);
                        }
                    }
                }
            }
        });
        if close {
            commands.push(Command::ToggleResults);
        }
        if let Some(target) = target {
            self.open_result(target);
        }
    }

    /// Jumps to the next (or previous) line in the results panel.
    pub(super) fn step_result(&mut self, forward: bool) {
        let Some(target) = self.results.as_mut().and_then(|r| r.step(forward)) else {
            return;
        };
        self.show_results = true;
        self.open_result(target);
    }

    fn open_result(&mut self, target: Target) {
        let Some(results) = &self.results else {
            return;
        };
        let index = match &results.groups[target.source].source {
            Source::Tab(id) => self.tab_index(*id),
            Source::File(path) => {
                let path = path.clone();
                self.open_path(&path);
                let canonical = path.canonicalize().ok();
                self.tabs
                    .iter()
                    .position(|tab| canonical.is_some() && tab.doc.path() == canonical.as_deref())
            }
        };
        let Some(index) = index else {
            return;
        };
        self.activate(index);
        let tab = &mut self.tabs[index];
        let rope = tab.doc.text();
        if target.line < rope.len_lines() {
            let start = rope.line_to_char(target.line);
            let len = rope.line(target.line).len_chars();
            let from = start + target.hit.start.min(len);
            let to = start + target.hit.end.min(len);
            tab.editor.set_selection(from, to);
        }
    }

    pub(super) fn side_panels(&mut self, ui: &mut Ui, commands: &mut Vec<Command>, chrome: bool) {
        if !chrome {
            return;
        }
        let texts = self.texts();
        if self.settings.get(Flag::FileTree) {
            self.file_tree_panel(ui, commands);
        }
        if self.settings.get(Flag::DocumentMap) && self.compare.is_none() {
            self.document_map_panel(ui);
        }
        if self.settings.get(Flag::DocumentList) {
            Panel::left("pilcrow-documents")
                .resizable(true)
                .default_size(180.0)
                .show(ui, |ui| {
                    ui.strong(texts.document_list);
                    ui.separator();
                    ScrollArea::vertical().show(ui, |ui| {
                        for (index, tab) in self.tabs.iter().enumerate() {
                            let marker = if tab.doc.is_modified() { " ●" } else { "" };
                            let label = format!("{}{marker}", tab.title(texts));
                            let response = ui.selectable_label(index == self.active, label);
                            if let Some(path) = tab.doc.path() {
                                response.clone().on_hover_text(path.display().to_string());
                            }
                            if response.clicked() {
                                commands.push(Command::Activate(index));
                            }
                        }
                    });
                });
        }
        let right = [
            Flag::FunctionList,
            Flag::ClipboardHistory,
            Flag::CharacterPanel,
        ]
        .into_iter()
        .any(|flag| self.settings.get(flag));
        if !right {
            return;
        }
        let insert = Panel::right("pilcrow-side")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                let mut insert = None;
                if self.settings.get(Flag::FunctionList) {
                    self.function_list(ui);
                }
                if self.settings.get(Flag::ClipboardHistory) {
                    insert = self.clipboard_panel(ui);
                }
                if self.settings.get(Flag::CharacterPanel) {
                    insert = insert.or(character_panel(ui, texts));
                }
                insert
            })
            .inner;
        if let Some(text) = insert {
            let tab = self.tab();
            if !tab.read_only {
                tab.editor.insert(&mut tab.doc, &text);
            }
            tab.editor.request_focus();
        }
    }

    fn function_list(&mut self, ui: &mut Ui) {
        let texts = self.texts();
        let tab = &mut self.tabs[self.active];
        let key = (tab.id, tab.doc.revision(), tab.syntax());
        if self.symbols.key != Some(key) {
            self.symbols.symbols = code::symbols(&tab.doc, tab.syntax());
            self.symbols.key = Some(key);
        }
        egui::CollapsingHeader::new(texts.function_list)
            .default_open(true)
            .show(ui, |ui| {
                if self.symbols.symbols.is_empty() {
                    ui.weak(texts.no_symbols);
                    return;
                }
                ScrollArea::vertical()
                    .id_salt("pilcrow-functions")
                    .max_height(ui.available_height() * 0.6)
                    .show(ui, |ui| {
                        for symbol in &self.symbols.symbols {
                            if ui.selectable_label(false, &symbol.name).clicked() {
                                let pos = tab.doc.text().line_to_char(symbol.line);
                                tab.editor.set_cursor(pos);
                                tab.editor.request_focus();
                            }
                        }
                    });
            });
    }

    /// Returns a text to insert, if one was clicked.
    fn clipboard_panel(&self, ui: &mut Ui) -> Option<String> {
        let texts = self.texts();
        let mut insert = None;
        egui::CollapsingHeader::new(texts.clipboard_history)
            .default_open(true)
            .show(ui, |ui| {
                if self.clipboard.is_empty() {
                    ui.weak(texts.clipboard_empty);
                }
                ScrollArea::vertical()
                    .id_salt("pilcrow-clipboard")
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for text in &self.clipboard {
                            let preview: String = text
                                .chars()
                                .take(60)
                                .map(|c| if c == '\n' { '⏎' } else { c })
                                .collect();
                            if ui.selectable_label(false, preview).clicked() {
                                insert = Some(text.clone());
                            }
                        }
                    });
            });
        insert
    }
}

/// Characters 32–255 with their codes; returns a clicked character.
fn character_panel(ui: &mut Ui, texts: &Texts) -> Option<String> {
    let mut insert = None;
    egui::CollapsingHeader::new(texts.character_panel)
        .default_open(true)
        .show(ui, |ui| {
            let codes: Vec<u32> = (32..127).chain(160..256).collect();
            let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
            ui.horizontal(|ui| {
                ui.monospace(format!(
                    "{:>5} {:>4}  {}",
                    texts.char_value, "Hex", texts.char_char
                ));
            });
            ScrollArea::vertical()
                .id_salt("pilcrow-characters")
                .show_rows(ui, row_height, codes.len(), |ui, rows| {
                    for &code in &codes[rows] {
                        let ch = char::from_u32(code).unwrap_or(' ');
                        let text = format!("{code:>5} {code:>4X}  {ch}");
                        if ui
                            .selectable_label(false, RichText::new(text).monospace())
                            .clicked()
                        {
                            insert = Some(ch.to_string());
                        }
                    }
                });
        });
    insert
}
