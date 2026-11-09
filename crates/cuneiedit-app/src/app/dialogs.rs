//! Modal dialogs. Each is shown until it returns a result; then the app acts on it.

use std::ops::RangeInclusive;

use cuneiedit_core::Search;
use cuneiedit_core::transform::find_char_in_range;
use cuneiedit_editor::Repeat;
use eframe::egui::{self, Id, Key, Modal, TextEdit, Ui};

use super::CuneiEditApp;
use super::file_search::FileSearch;
use super::panels::Results;
use super::preferences::{LanguagesForm, PreferencesForm};
use crate::i18n::fill;
use crate::macros::{self, SavedMacro};

#[derive(Clone, Copy)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharPreset {
    NonAscii,
    Ascii,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumberBase {
    #[default]
    Dec,
    Hex,
    Oct,
    Bin,
}

/// Input of the column editor.
#[derive(Clone, Debug)]
pub struct ColumnForm {
    numbers: bool,
    text: String,
    initial: String,
    increment: String,
    repeat: String,
    leading_zeros: bool,
    base: NumberBase,
    focus: bool,
}

impl Default for ColumnForm {
    fn default() -> Self {
        Self {
            numbers: false,
            text: String::new(),
            initial: "1".into(),
            increment: "1".into(),
            repeat: "1".into(),
            leading_zeros: false,
            base: NumberBase::Dec,
            focus: true,
        }
    }
}

/// `count` numbers from `initial`, each repeated `repeat` times, right-aligned to one width.
pub fn column_numbers(
    count: usize,
    initial: i64,
    increment: i64,
    repeat: usize,
    base: NumberBase,
    leading_zeros: bool,
) -> Vec<String> {
    let formatted: Vec<(bool, String)> = (0..count)
        .map(|i| {
            let step = (i / repeat.max(1)) as i64;
            let value = initial.saturating_add(increment.saturating_mul(step));
            let magnitude = value.unsigned_abs();
            let digits = match base {
                NumberBase::Dec => magnitude.to_string(),
                NumberBase::Hex => format!("{magnitude:X}"),
                NumberBase::Oct => format!("{magnitude:o}"),
                NumberBase::Bin => format!("{magnitude:b}"),
            };
            (value < 0, digits)
        })
        .collect();
    let width = formatted
        .iter()
        .map(|(negative, digits)| digits.len() + usize::from(*negative))
        .max()
        .unwrap_or(0);
    formatted
        .into_iter()
        .map(|(negative, digits)| {
            let sign = if negative { "-" } else { "" };
            let pad = width - digits.len() - sign.len();
            if leading_zeros {
                format!("{sign}{}{digits}", "0".repeat(pad))
            } else {
                format!("{}{sign}{digits}", " ".repeat(pad))
            }
        })
        .collect()
}

pub enum Dialog {
    Unsaved {
        tab: u64,
    },
    Message {
        title: String,
        text: String,
    },
    GoTo {
        input: String,
        offset: bool,
        focus: bool,
    },
    Rename {
        tab: u64,
        name: String,
        focus: bool,
    },
    ConfirmTrash {
        tab: u64,
    },
    DiskChanged {
        tab: u64,
        deleted: bool,
    },
    FindInFiles {
        focus: bool,
    },
    ConfirmReplaceInFiles,
    CharRange {
        preset: CharPreset,
        from: String,
        to: String,
        status: Option<String>,
    },
    PlayMacro {
        times: String,
        until_end: bool,
    },
    Checksums {
        title: String,
        lines: Vec<(String, String)>,
    },
    Preferences(Box<PreferencesForm>),
    UserLanguages(Box<LanguagesForm>),
    ColumnEditor(ColumnForm),
    SaveMacro {
        name: String,
        focus: bool,
    },
    UpdateConsent,
    Update,
}

/// Buttons in a row; returns the index of the clicked one.
fn buttons(ui: &mut Ui, labels: &[&str]) -> Option<usize> {
    let mut clicked = None;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for (index, label) in labels.iter().enumerate() {
            if ui.button(*label).clicked() {
                clicked = Some(index);
            }
        }
    });
    clicked
}

fn modal<R>(ctx: &egui::Context, id: &str, content: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    modal_sized(ctx, id, 460.0, content)
}

pub fn modal_sized<R>(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    content: impl FnOnce(&mut Ui) -> R,
) -> (R, bool) {
    let response = Modal::new(Id::unique(id)).show(ctx, |ui| {
        ui.set_max_width(width);
        content(ui)
    });
    let escaped = response.should_close();
    (response.inner, escaped)
}

/// Buttons in a row; returns the index of the clicked one.
pub fn dialog_buttons(ui: &mut Ui, labels: &[&str]) -> Option<usize> {
    buttons(ui, labels)
}

impl CuneiEditApp {
    pub(super) fn show_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let texts = self.texts();
        let keep = match dialog {
            Dialog::Unsaved { tab } => {
                let name = self
                    .tab_index(tab)
                    .map(|index| self.tabs[index].title(texts))
                    .unwrap_or_default();
                let (clicked, escaped) = modal(ctx, "cuneiedit-unsaved", |ui| {
                    ui.heading(texts.unsaved_title);
                    ui.label(fill(texts.unsaved_question, &[("name", &name)]));
                    buttons(ui, &[texts.save, texts.dont_save, texts.cancel])
                });
                let choice = match clicked {
                    Some(0) => Some(UnsavedChoice::Save),
                    Some(1) => Some(UnsavedChoice::Discard),
                    Some(_) => Some(UnsavedChoice::Cancel),
                    None if escaped => Some(UnsavedChoice::Cancel),
                    None => None,
                };
                match choice {
                    Some(choice) => {
                        self.resolve_unsaved(ctx, tab, choice);
                        None
                    }
                    None => Some(Dialog::Unsaved { tab }),
                }
            }
            Dialog::Message { title, text } => {
                let (clicked, escaped) = modal(ctx, "cuneiedit-message", |ui| {
                    ui.heading(&title);
                    ui.label(&text);
                    buttons(ui, &[texts.ok])
                });
                (clicked.is_none() && !escaped).then_some(Dialog::Message { title, text })
            }
            Dialog::GoTo {
                input,
                offset,
                focus,
            } => self.go_to_dialog(ctx, input, offset, focus),
            Dialog::Rename { tab, name, focus } => {
                let mut name = name;
                let (clicked, escaped) = modal(ctx, "cuneiedit-rename", |ui| {
                    ui.heading(texts.rename.trim_end_matches('…'));
                    ui.label(texts.new_name);
                    let field = ui.add(TextEdit::singleline(&mut name).desired_width(320.0));
                    if focus {
                        field.request_focus();
                    }
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    buttons(ui, &[texts.ok, texts.cancel]).or(enter.then_some(0))
                });
                match clicked {
                    Some(0) => {
                        self.rename_tab(tab, &name);
                        None
                    }
                    Some(_) => None,
                    None if escaped => None,
                    None => Some(Dialog::Rename {
                        tab,
                        name,
                        focus: false,
                    }),
                }
            }
            Dialog::ConfirmTrash { tab } => {
                let name = self
                    .tab_index(tab)
                    .map(|index| self.tabs[index].title(texts))
                    .unwrap_or_default();
                let (clicked, escaped) = modal(ctx, "cuneiedit-trash", |ui| {
                    ui.label(fill(texts.trash_question, &[("name", &name)]));
                    buttons(
                        ui,
                        &[texts.move_to_trash.trim_end_matches('…'), texts.cancel],
                    )
                });
                match clicked {
                    Some(0) => {
                        self.trash_tab(tab);
                        None
                    }
                    Some(_) => None,
                    None if escaped => None,
                    None => Some(Dialog::ConfirmTrash { tab }),
                }
            }
            Dialog::DiskChanged { tab, deleted } => {
                let Some(index) = self.tab_index(tab) else {
                    return;
                };
                let name = self.tabs[index].title(texts);
                let modified = self.tabs[index].doc.is_modified();
                let (clicked, escaped) = modal(ctx, "cuneiedit-disk", |ui| {
                    if deleted {
                        ui.label(fill(texts.deleted_on_disk, &[("name", &name)]));
                        buttons(ui, &[texts.keep, texts.close])
                    } else {
                        ui.label(fill(texts.changed_on_disk, &[("name", &name)]));
                        if modified {
                            ui.label(texts.undo_reload_hint);
                        }
                        buttons(ui, &[texts.reload_button, texts.keep])
                    }
                });
                let accept = match clicked {
                    Some(index) => Some(index == 0),
                    // Escape keeps the current text in both cases.
                    None if escaped => Some(deleted),
                    None => None,
                };
                match accept {
                    Some(accept) => {
                        self.resolve_disk_change(ctx, tab, deleted, accept);
                        None
                    }
                    None => Some(Dialog::DiskChanged { tab, deleted }),
                }
            }
            Dialog::FindInFiles { focus } => self.find_in_files_dialog(ctx, focus),
            Dialog::ConfirmReplaceInFiles => {
                let dir = self.files_form.dir.clone();
                let (clicked, escaped) = modal(ctx, "cuneiedit-replace-files", |ui| {
                    ui.label(fill(texts.confirm_replace_files, &[("dir", &dir)]));
                    buttons(ui, &[texts.replace_all, texts.cancel])
                });
                match clicked {
                    Some(0) => {
                        self.start_file_search(true);
                        None
                    }
                    Some(_) => None,
                    None if escaped => None,
                    None => Some(Dialog::ConfirmReplaceInFiles),
                }
            }
            Dialog::CharRange {
                preset,
                from,
                to,
                status,
            } => self.char_range_dialog(ctx, preset, from, to, status),
            Dialog::PlayMacro { times, until_end } => {
                let (mut times, mut until_end) = (times, until_end);
                let (clicked, escaped) = modal(ctx, "cuneiedit-macro", |ui| {
                    ui.heading(texts.play_repeatedly.trim_end_matches('…'));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut until_end, false, texts.run_times);
                        ui.add_enabled(
                            !until_end,
                            TextEdit::singleline(&mut times).desired_width(60.0),
                        );
                    });
                    ui.radio_value(&mut until_end, true, texts.until_end);
                    buttons(ui, &[texts.run, texts.cancel])
                });
                let count = times.trim().parse::<usize>().ok().filter(|&n| n > 0);
                match clicked {
                    Some(0) if until_end || count.is_some() => {
                        let repeat = if until_end {
                            Repeat::UntilEnd
                        } else {
                            Repeat::Times(count.unwrap_or(1))
                        };
                        let events = self.macro_events.clone();
                        self.tab().editor.play(events, repeat);
                        None
                    }
                    Some(1) => None,
                    None if escaped => None,
                    _ => Some(Dialog::PlayMacro { times, until_end }),
                }
            }
            Dialog::Checksums { title, lines } => {
                let (clicked, escaped) = modal(ctx, "cuneiedit-checksums", |ui| {
                    ui.heading(&title);
                    egui::Grid::new("cuneiedit-checksum-grid")
                        .striped(true)
                        .show(ui, |ui| {
                            for (name, hash) in &lines {
                                ui.label(name);
                                ui.monospace(hash);
                                ui.end_row();
                            }
                        });
                    buttons(ui, &[texts.copy, texts.ok])
                });
                if clicked == Some(0) {
                    let text: Vec<String> = lines
                        .iter()
                        .map(|(name, hash)| format!("{hash}  {name}"))
                        .collect();
                    ctx.copy_text(text.join("\n"));
                }
                (clicked.is_none() && !escaped).then_some(Dialog::Checksums { title, lines })
            }
            Dialog::Preferences(form) => self.preferences_dialog(ctx, form),
            Dialog::UserLanguages(form) => self.languages_dialog(ctx, form),
            Dialog::ColumnEditor(form) => self.column_dialog(ctx, form),
            Dialog::SaveMacro { name, focus } => {
                let mut name = name;
                let (clicked, escaped) = modal(ctx, "cuneiedit-save-macro", |ui| {
                    ui.heading(texts.save_macro.trim_end_matches('…'));
                    ui.label(texts.macro_name);
                    let field = ui.add(TextEdit::singleline(&mut name).desired_width(320.0));
                    if focus {
                        field.request_focus();
                    }
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    buttons(ui, &[texts.save, texts.cancel]).or(enter.then_some(0))
                });
                let name = name.trim().to_owned();
                match clicked {
                    Some(0) if !name.is_empty() => {
                        self.saved_macros.retain(|saved| saved.name != name);
                        self.saved_macros.push(SavedMacro {
                            name,
                            events: self.macro_events.clone(),
                        });
                        if let Err(err) = macros::save(&self.saved_macros) {
                            self.error(err.to_string());
                        }
                        None
                    }
                    Some(1) => None,
                    None if escaped => None,
                    _ => Some(Dialog::SaveMacro { name, focus: false }),
                }
            }
            Dialog::UpdateConsent => self.update_consent_dialog(ctx),
            Dialog::Update => self.update_dialog(ctx),
        };
        // A dialog opened while handling this one takes precedence.
        if self.dialog.is_none() {
            self.dialog = keep;
        }
        if self.dialog.is_none() {
            self.tab().editor.request_focus();
        }
    }

    fn go_to_dialog(
        &mut self,
        ctx: &egui::Context,
        mut input: String,
        mut offset: bool,
        focus: bool,
    ) -> Option<Dialog> {
        let texts = self.texts();
        let rope = self.tabs[self.active].doc.text();
        let (lines, chars) = (rope.len_lines(), rope.len_chars());
        let mut go = false;
        let (clicked, escaped) = modal(ctx, "cuneiedit-go-to", |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut offset, false, texts.go_line);
                ui.radio_value(&mut offset, true, texts.go_offset);
            });
            let label = if offset {
                fill(texts.offset_number, &[("max", &chars.to_string())])
            } else {
                fill(texts.line_number, &[("max", &lines.to_string())])
            };
            ui.label(label);
            let field = ui.add(TextEdit::singleline(&mut input).desired_width(220.0));
            if focus {
                field.request_focus();
            }
            go = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            buttons(ui, &[texts.go, texts.cancel])
        });
        go |= clicked == Some(0);
        let number = input.trim().parse::<usize>().ok();
        let target = if offset {
            number.filter(|&n| n <= chars)
        } else {
            number
                .filter(|line| (1..=lines).contains(line))
                .map(|line| rope.line_to_char(line - 1))
        };
        match target {
            Some(pos) if go => {
                self.tab().editor.set_cursor(pos);
                None
            }
            _ if clicked == Some(1) || escaped => None,
            _ => Some(Dialog::GoTo {
                input,
                offset,
                focus: go,
            }),
        }
    }

    fn find_in_files_dialog(&mut self, ctx: &egui::Context, focus: bool) -> Option<Dialog> {
        let texts = self.texts();
        let (clicked, escaped) = modal(ctx, "cuneiedit-find-files", |ui| {
            ui.heading(texts.find_in_files.trim_end_matches('…'));
            egui::Grid::new("cuneiedit-find-files-grid")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(texts.find_label);
                    let field = ui.add(
                        TextEdit::singleline(&mut self.find.query_mut().pattern)
                            .desired_width(320.0),
                    );
                    if focus {
                        field.request_focus();
                    }
                    ui.end_row();
                    ui.label(texts.replace_label);
                    ui.add(TextEdit::singleline(self.find.replacement_mut()).desired_width(320.0));
                    ui.end_row();
                    ui.label(texts.filters);
                    ui.add(
                        TextEdit::singleline(&mut self.files_form.filters)
                            .hint_text("*.txt *.rs !*.bak")
                            .desired_width(320.0),
                    );
                    ui.end_row();
                    ui.label(texts.directory);
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(&mut self.files_form.dir).desired_width(230.0));
                        if ui.button(texts.browse).clicked()
                            && let Some(dir) = rfd::FileDialog::new()
                                .set_directory(&self.files_form.dir)
                                .pick_folder()
                        {
                            self.files_form.dir = dir.display().to_string();
                        }
                    });
                    ui.end_row();
                });
            let query = self.find.query_mut();
            ui.checkbox(&mut query.match_case, texts.match_case);
            ui.checkbox(&mut query.whole_word, texts.whole_word);
            ui.checkbox(&mut query.regex, texts.regex);
            ui.checkbox(&mut query.extended, texts.extended);
            ui.checkbox(&mut self.files_form.subfolders, texts.subfolders);
            ui.checkbox(&mut self.files_form.hidden, texts.hidden_folders);
            buttons(
                ui,
                &[texts.find_all_button, texts.replace_in_files, texts.cancel],
            )
        });
        let ready = self.find.has_pattern() && !self.files_form.dir.trim().is_empty();
        match clicked {
            Some(0) if ready => {
                self.start_file_search(false);
                None
            }
            Some(1) if ready => Some(Dialog::ConfirmReplaceInFiles),
            Some(2) => None,
            None if escaped => None,
            _ => Some(Dialog::FindInFiles { focus: false }),
        }
    }

    /// Starts finding (or replacing) in files with the current query.
    pub(super) fn start_file_search(&mut self, replace: bool) {
        let search = match Search::new(self.find.query()) {
            Ok(Some(search)) => search,
            Ok(None) => return,
            Err(err) => {
                let message = format!("{}: {err}", self.texts().invalid_regex);
                self.error(message);
                return;
            }
        };
        let replacement = replace.then(|| self.find.replacement().to_owned());
        let title = format!(
            "“{}” – {}",
            self.find.query().pattern,
            self.files_form.dir.trim()
        );
        self.results = Some(Results::new(title));
        self.show_results = true;
        self.file_search = Some(FileSearch::start(&self.files_form, search, replacement));
    }

    fn char_range_dialog(
        &mut self,
        ctx: &egui::Context,
        mut preset: CharPreset,
        mut from: String,
        mut to: String,
        status: Option<String>,
    ) -> Option<Dialog> {
        let texts = self.texts();
        let (clicked, escaped) = modal(ctx, "cuneiedit-char-range", |ui| {
            ui.heading(texts.find_chars.trim_end_matches('…'));
            ui.radio_value(&mut preset, CharPreset::NonAscii, texts.non_ascii);
            ui.radio_value(&mut preset, CharPreset::Ascii, texts.ascii);
            ui.horizontal(|ui| {
                ui.radio_value(&mut preset, CharPreset::Custom, texts.custom_range);
                let enabled = preset == CharPreset::Custom;
                ui.add_enabled(enabled, TextEdit::singleline(&mut from).desired_width(70.0));
                ui.label("–");
                ui.add_enabled(enabled, TextEdit::singleline(&mut to).desired_width(70.0));
            });
            if let Some(status) = &status {
                ui.colored_label(ui.visuals().warn_fg_color, status);
            }
            buttons(ui, &[texts.find_next, texts.find_previous, texts.close])
        });
        let range: Option<RangeInclusive<u32>> = match preset {
            CharPreset::NonAscii => Some(128..=char::MAX as u32),
            CharPreset::Ascii => Some(0..=127),
            CharPreset::Custom => match (from.trim().parse(), to.trim().parse()) {
                (Ok(a), Ok(b)) if a <= b => Some(a..=b),
                _ => None,
            },
        };
        let mut status = status;
        match clicked {
            Some(index @ (0 | 1)) => {
                let forward = index == 0;
                status = Some(texts.not_found.to_owned());
                if let Some(range) = range {
                    let tab = self.tab();
                    let selection = tab.editor.selection();
                    let start = if forward {
                        selection.end
                    } else {
                        selection.start
                    };
                    if let Some(pos) = find_char_in_range(tab.doc.text(), start, forward, &range) {
                        tab.editor.set_selection(pos, pos + 1);
                        status = None;
                    }
                }
            }
            Some(_) => return None,
            None if escaped => return None,
            None => {}
        }
        Some(Dialog::CharRange {
            preset,
            from,
            to,
            status,
        })
    }

    fn column_dialog(&mut self, ctx: &egui::Context, mut form: ColumnForm) -> Option<Dialog> {
        let texts = self.texts();
        let (clicked, escaped) = modal(ctx, "cuneiedit-column-editor", |ui| {
            ui.heading(texts.column_editor.trim_end_matches('…'));
            ui.radio_value(&mut form.numbers, false, texts.column_text);
            let field = ui.add_enabled(
                !form.numbers,
                TextEdit::singleline(&mut form.text).desired_width(320.0),
            );
            if form.focus {
                field.request_focus();
            }
            ui.add_space(6.0);
            ui.radio_value(&mut form.numbers, true, texts.column_numbers);
            ui.add_enabled_ui(form.numbers, |ui| {
                egui::Grid::new("cuneiedit-column-grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        let fields = [
                            (texts.column_initial, &mut form.initial),
                            (texts.column_increment, &mut form.increment),
                            (texts.column_repeat, &mut form.repeat),
                        ];
                        for (label, value) in fields {
                            ui.label(label);
                            ui.add(TextEdit::singleline(value).desired_width(80.0));
                            ui.end_row();
                        }
                        ui.label(texts.column_format);
                        ui.horizontal(|ui| {
                            let bases = [
                                (NumberBase::Dec, texts.format_dec),
                                (NumberBase::Hex, texts.format_hex),
                                (NumberBase::Oct, texts.format_oct),
                                (NumberBase::Bin, texts.format_bin),
                            ];
                            for (base, label) in bases {
                                ui.radio_value(&mut form.base, base, label);
                            }
                        });
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut form.leading_zeros, texts.column_leading_zeros);
                        ui.end_row();
                    });
            });
            ui.add_space(4.0);
            ui.small(texts.column_editor_help);
            buttons(ui, &[texts.ok, texts.cancel])
        });
        form.focus = false;
        match clicked {
            Some(0) if self.apply_column(&form) => None,
            Some(1) => None,
            None if escaped => None,
            _ => Some(Dialog::ColumnEditor(form)),
        }
    }

    /// Inserts the column editor's text or numbers. Returns false for invalid numbers.
    fn apply_column(&mut self, form: &ColumnForm) -> bool {
        let numbers = if form.numbers {
            let parsed = (
                form.initial.trim().parse::<i64>(),
                form.increment.trim().parse::<i64>(),
                form.repeat.trim().parse::<usize>(),
            );
            let (Ok(initial), Ok(increment), Ok(repeat)) = parsed else {
                return false;
            };
            Some((initial, increment, repeat))
        } else {
            None
        };
        let tab_width = self.settings.tab_width;
        let tab = self.tab();
        let missing = if tab.editor.has_multiple_selections() {
            vec![0; tab.editor.selections().len()]
        } else {
            tab.editor.column_cursors(&tab.doc, tab_width)
        };
        let values = match numbers {
            Some((initial, increment, repeat)) => column_numbers(
                missing.len(),
                initial,
                increment,
                repeat,
                form.base,
                form.leading_zeros,
            ),
            None => vec![form.text.clone(); missing.len()],
        };
        tab.editor.insert_each(&mut tab.doc, |index| {
            format!("{}{}", " ".repeat(missing[index]), values[index])
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_column_numbers() {
        let dec = |count, initial, increment, repeat, zeros| {
            column_numbers(count, initial, increment, repeat, NumberBase::Dec, zeros)
        };
        assert_eq!(dec(4, 8, 1, 1, false), vec![" 8", " 9", "10", "11"]);
        assert_eq!(dec(4, 8, 1, 1, true), vec!["08", "09", "10", "11"]);
        assert_eq!(dec(4, 1, 1, 2, false), vec!["1", "1", "2", "2"]);
        assert_eq!(dec(3, 1, -1, 1, true), vec!["01", "00", "-1"]);
        assert_eq!(
            column_numbers(3, 9, 1, 1, NumberBase::Hex, false),
            vec!["9", "A", "B"]
        );
        assert_eq!(
            column_numbers(2, 2, 1, 1, NumberBase::Bin, true),
            vec!["10", "11"]
        );
    }
}
