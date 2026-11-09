//! The settings dialog (general, editor, backup, shortcuts) and the dialog for user-defined
//! languages.

use cuneiedit_core::highlight::{self, Language as Syntax, UserLanguage};
use cuneiedit_core::{Encoding, LineEnding};
use eframe::egui::os::OperatingSystem;
use eframe::egui::{
    self, ComboBox, DragValue, Event, Key, KeyboardShortcut, ScrollArea, TextEdit, ThemePreference,
    Ui,
};

use super::CuneiEditApp;
use super::command::{CUSTOMIZABLE, Command, SHORTCUTS, flag_label};
use super::dialogs::{Dialog, dialog_buttons, modal_sized};
use super::files::apply_font;
use super::keymap::{self, Keymap, command_id};
use crate::i18n::{Language, Texts};
use crate::languages;
use crate::settings::{Flag, SaveBackup, Settings, UpdateMode};
use crate::themes::THEMES;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Editor,
    Backup,
    Shortcuts,
}

/// The settings being edited; applied with OK.
pub struct PreferencesForm {
    page: Page,
    settings: Settings,
    date_format: String,
    edge_column: String,
    filter: String,
    /// Command waiting for a key press.
    recording: Option<Command>,
    error: Option<String>,
}

/// Sets the shortcut text for `command` in `changes` (empty removes the shortcut).
fn set_change(changes: &mut Vec<(String, String)>, command: Command, text: String) {
    let id = command_id(command);
    match changes.iter_mut().find(|(changed, _)| *changed == id) {
        Some(entry) => entry.1 = text,
        None => changes.push((id, text)),
    }
}

/// Gives `command` the `shortcut`, taking it away from the command that had it.
fn assign(changes: &mut Vec<(String, String)>, command: Command, shortcut: KeyboardShortcut) {
    if let Some(other) = Keymap::new(changes).command_for(&shortcut)
        && other != command
    {
        set_change(changes, other, String::new());
    }
    set_change(changes, command, keymap::format(&shortcut));
}

/// Every command that can have a shortcut, once each.
fn shortcut_commands() -> Vec<Command> {
    let mut commands: Vec<Command> = Vec::new();
    for &command in SHORTCUTS.iter().map(|(_, c)| c).chain(CUSTOMIZABLE) {
        if !commands.contains(&command) {
            commands.push(command);
        }
    }
    commands
}

fn flag_box(ui: &mut Ui, settings: &mut Settings, flag: Flag, texts: &Texts) {
    let mut on = settings.get(flag);
    if ui.checkbox(&mut on, flag_label(flag, texts)).changed() {
        settings.set(flag, on);
    }
}

/// The fields of one user-defined language, as typed.
#[derive(Clone, Default)]
struct LanguageFields {
    name: String,
    extensions: String,
    keywords: String,
    types: String,
    line_comment: String,
    block_start: String,
    block_end: String,
    quotes: String,
    ignore_case: bool,
}

impl From<&UserLanguage> for LanguageFields {
    fn from(language: &UserLanguage) -> Self {
        let (block_start, block_end) = language.block_comment.clone().unwrap_or_default();
        Self {
            name: language.name.clone(),
            extensions: language.extensions.join(" "),
            keywords: language.keywords.join(" "),
            types: language.types.join(" "),
            line_comment: language.line_comment.clone(),
            block_start,
            block_end,
            quotes: language.quotes.clone(),
            ignore_case: language.ignore_case,
        }
    }
}

impl LanguageFields {
    fn to_language(&self) -> UserLanguage {
        let block = (self.block_start.trim(), self.block_end.trim());
        UserLanguage {
            name: self.name.trim().to_owned(),
            extensions: languages::words(&self.extensions),
            keywords: languages::words(&self.keywords),
            types: languages::words(&self.types),
            line_comment: self.line_comment.trim().to_owned(),
            block_comment: (!block.0.is_empty() && !block.1.is_empty())
                .then(|| (block.0.to_owned(), block.1.to_owned())),
            quotes: self.quotes.chars().filter(|c| !c.is_whitespace()).collect(),
            ignore_case: self.ignore_case,
        }
    }
}

pub struct LanguagesForm {
    languages: Vec<LanguageFields>,
    selected: usize,
}

impl CuneiEditApp {
    pub(super) fn open_preferences(&mut self) {
        let settings = self.settings.clone();
        self.dialog = Some(Dialog::Preferences(Box::new(PreferencesForm {
            page: Page::General,
            date_format: settings.date_format.clone(),
            edge_column: settings.edge_column.to_string(),
            settings,
            filter: String::new(),
            recording: None,
            error: None,
        })));
    }

    pub(super) fn preferences_dialog(
        &mut self,
        ctx: &egui::Context,
        mut form: Box<PreferencesForm>,
    ) -> Option<Dialog> {
        let texts = self.texts();
        if let Some(command) = form.recording {
            // Take the key press before the dialog (or a text field) sees it.
            let pressed = ctx.input_mut(|input| {
                let found = input.events.iter().find_map(|event| match event {
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                });
                if found.is_some() {
                    input
                        .events
                        .retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_)));
                }
                found
            });
            match pressed {
                Some((Key::Escape, modifiers)) if modifiers.is_none() => form.recording = None,
                Some((key, modifiers)) => {
                    let mac = ctx.os() == OperatingSystem::Mac;
                    let shortcut = keymap::from_press(key, modifiers, mac);
                    assign(&mut form.settings.shortcuts, command, shortcut);
                    form.recording = None;
                }
                None => {}
            }
        }

        let (clicked, escaped) = modal_sized(ctx, "cuneiedit-preferences", 620.0, |ui| {
            ui.set_min_width(560.0);
            ui.heading(texts.preferences.trim_end_matches('…'));
            ui.horizontal(|ui| {
                let pages = [
                    (Page::General, texts.tab_general),
                    (Page::Editor, texts.tab_editor),
                    (Page::Backup, texts.tab_backup),
                    (Page::Shortcuts, texts.tab_shortcuts),
                ];
                for (page, label) in pages {
                    ui.selectable_value(&mut form.page, page, label);
                }
            });
            ui.separator();
            match form.page {
                Page::General => general_page(ui, &mut form, texts),
                Page::Editor => editor_page(ui, &mut form, texts),
                Page::Backup => backup_page(ui, &mut form, texts),
                Page::Shortcuts => shortcuts_page(ui, &mut form, texts),
            }
            if let Some(error) = &form.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            dialog_buttons(ui, &[texts.ok, texts.cancel])
        });
        match clicked {
            Some(0) => {
                let column = form
                    .edge_column
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|c| (1..=1000).contains(c));
                let valid_format = super::run::format_now(&form.date_format).is_some();
                match column {
                    Some(column) if valid_format => {
                        let mut settings = form.settings;
                        settings.date_format = form.date_format;
                        settings.edge_column = column;
                        self.apply_settings(ctx, settings);
                        return None;
                    }
                    _ if !valid_format => form.error = Some(texts.invalid_date_format.to_owned()),
                    _ => form.error = Some(format!("{}: 1–1000", texts.edge_column)),
                }
            }
            Some(_) => return None,
            None if escaped && form.recording.is_none() => return None,
            None => {}
        }
        Some(Dialog::Preferences(form))
    }

    fn apply_settings(&mut self, ctx: &egui::Context, settings: Settings) {
        let old_font = self.settings.font_path.clone();
        self.settings = settings;
        ctx.set_theme(self.settings.theme);
        self.keymap = Keymap::new(&self.settings.shortcuts);
        if self.settings.font_path != old_font
            && let Err(err) = apply_font(ctx, self.settings.font_path.as_deref())
        {
            self.settings.font_path = old_font;
            let _ = apply_font(ctx, self.settings.font_path.as_deref());
            let message = err.message(self.texts());
            self.error(message);
        }
        self.persist();
    }

    pub(super) fn open_user_languages(&mut self) {
        let languages: Vec<LanguageFields> = self
            .user_languages
            .iter()
            .map(LanguageFields::from)
            .collect();
        self.dialog = Some(Dialog::UserLanguages(Box::new(LanguagesForm {
            languages,
            selected: 0,
        })));
    }

    pub(super) fn languages_dialog(
        &mut self,
        ctx: &egui::Context,
        mut form: Box<LanguagesForm>,
    ) -> Option<Dialog> {
        let texts = self.texts();
        let (clicked, escaped) = modal_sized(ctx, "cuneiedit-languages", 640.0, |ui| {
            ui.set_min_width(600.0);
            ui.heading(texts.user_languages.trim_end_matches('…'));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    ScrollArea::vertical()
                        .id_salt("cuneiedit-language-list")
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for (index, language) in form.languages.iter().enumerate() {
                                let name = if language.name.trim().is_empty() {
                                    texts.udl_new
                                } else {
                                    language.name.as_str()
                                };
                                if ui.selectable_label(index == form.selected, name).clicked() {
                                    form.selected = index;
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui.button(texts.add).clicked() {
                            form.languages.push(LanguageFields {
                                quotes: "\"'".into(),
                                ..LanguageFields::default()
                            });
                            form.selected = form.languages.len() - 1;
                        }
                        let can_delete = form.selected < form.languages.len();
                        if ui
                            .add_enabled(can_delete, egui::Button::new(texts.delete))
                            .clicked()
                        {
                            form.languages.remove(form.selected);
                            form.selected = form.selected.saturating_sub(1);
                        }
                    });
                });
                ui.separator();
                if let Some(fields) = form.languages.get_mut(form.selected) {
                    language_fields(ui, fields, texts);
                }
            });
            dialog_buttons(ui, &[texts.save, texts.cancel])
        });
        match clicked {
            Some(0) => {
                let defined: Vec<UserLanguage> = form
                    .languages
                    .iter()
                    .map(LanguageFields::to_language)
                    .filter(|language| !language.name.is_empty())
                    .collect();
                self.set_user_languages(defined);
                None
            }
            Some(_) => None,
            None if escaped => None,
            None => Some(Dialog::UserLanguages(form)),
        }
    }

    /// Replaces the user-defined languages and updates documents that use one.
    fn set_user_languages(&mut self, defined: Vec<UserLanguage>) {
        let used: Vec<Option<String>> = self
            .tabs
            .iter()
            .map(|tab| {
                let syntax = tab.syntax();
                syntax.is_user_defined().then(|| syntax.name().to_owned())
            })
            .collect();
        highlight::set_user_languages(&defined);
        for (tab, name) in self.tabs.iter_mut().zip(used) {
            if let Some(name) = name {
                let syntax = Syntax::from_name(&name).unwrap_or(Syntax::PLAIN_TEXT);
                tab.highlighter.set_language(syntax);
                tab.highlighter.reset();
            }
        }
        if let Err(err) = languages::save(&defined) {
            self.error(err.to_string());
        }
        self.user_languages = defined;
    }
}

fn general_page(ui: &mut Ui, form: &mut PreferencesForm, texts: &Texts) {
    let settings = &mut form.settings;
    egui::Grid::new("cuneiedit-general-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(texts.language);
            ComboBox::from_id_salt("cuneiedit-ui-language")
                .selected_text(settings.language.native_name())
                .show_ui(ui, |ui| {
                    for language in Language::ALL {
                        ui.selectable_value(
                            &mut settings.language,
                            language,
                            language.native_name(),
                        );
                    }
                });
            ui.end_row();

            ui.label(texts.theme);
            ui.horizontal(|ui| {
                let themes = [
                    (ThemePreference::System, texts.theme_system),
                    (ThemePreference::Light, texts.theme_light),
                    (ThemePreference::Dark, texts.theme_dark),
                ];
                for (theme, label) in themes {
                    ui.radio_value(&mut settings.theme, theme, label);
                }
            });
            ui.end_row();

            ui.label(texts.color_theme);
            let current = if settings.color_theme.is_empty() {
                texts.theme_standard
            } else {
                settings.color_theme.as_str()
            };
            ComboBox::from_id_salt("cuneiedit-color-theme")
                .selected_text(current.to_owned())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.color_theme,
                        String::new(),
                        texts.theme_standard,
                    );
                    for theme in THEMES {
                        ui.selectable_value(
                            &mut settings.color_theme,
                            theme.name.to_owned(),
                            theme.name,
                        );
                    }
                });
            ui.end_row();

            ui.label(texts.font);
            ui.horizontal(|ui| {
                let name = settings
                    .font_path
                    .as_deref()
                    .and_then(|p| p.file_name())
                    .map_or_else(
                        || texts.font_builtin.to_owned(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                ui.label(name);
                if ui.button(texts.choose_font).clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Font", &["ttf", "otf", "ttc"])
                        .pick_file()
                {
                    settings.font_path = Some(path);
                }
                if settings.font_path.is_some() && ui.button(texts.reset).clicked() {
                    settings.font_path = None;
                }
            });
            ui.end_row();

            ui.label(texts.font_size);
            let mut size = settings.font_size;
            if ui
                .add(DragValue::new(&mut size).range(6.0..=72.0).speed(0.2))
                .changed()
            {
                settings.set_font_size(size);
            }
            ui.end_row();
        });
}

fn editor_page(ui: &mut Ui, form: &mut PreferencesForm, texts: &Texts) {
    egui::Grid::new("cuneiedit-editor-grid")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label(texts.tab_width);
            ui.add(DragValue::new(&mut form.settings.tab_width).range(1..=16));
            ui.end_row();
            ui.label(texts.edge_column);
            ui.add(TextEdit::singleline(&mut form.edge_column).desired_width(60.0));
            ui.end_row();
            ui.label(texts.date_format);
            ui.add(TextEdit::singleline(&mut form.date_format).desired_width(220.0));
            ui.end_row();
            ui.label("");
            ui.small(texts.date_format_help);
            ui.end_row();
            ui.label(texts.new_documents);
            let settings = &mut form.settings;
            ui.horizontal(|ui| {
                ComboBox::from_id_salt("cuneiedit-new-encoding")
                    .selected_text(settings.new_encoding.label())
                    .show_ui(ui, |ui| {
                        for option in Encoding::ALL {
                            ui.selectable_value(&mut settings.new_encoding, option, option.label());
                        }
                    });
                ComboBox::from_id_salt("cuneiedit-new-line-ending")
                    .selected_text(settings.new_line_ending.label())
                    .show_ui(ui, |ui| {
                        for option in LineEnding::ALL {
                            ui.selectable_value(
                                &mut settings.new_line_ending,
                                option,
                                option.label(),
                            );
                        }
                    });
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    let flags = [
        Flag::IndentWithSpaces,
        Flag::AutoClose,
        Flag::AutoComplete,
        Flag::SmartHighlight,
        Flag::Folding,
        Flag::WordWrap,
        Flag::IndentGuides,
        Flag::ShowWhitespace,
        Flag::ShowLineEndings,
        Flag::EdgeLine,
        Flag::ChangeHistory,
    ];
    ui.columns(2, |columns| {
        for (index, flag) in flags.into_iter().enumerate() {
            flag_box(&mut columns[index % 2], &mut form.settings, flag, texts);
        }
    });
}

fn backup_page(ui: &mut Ui, form: &mut PreferencesForm, texts: &Texts) {
    flag_box(ui, &mut form.settings, Flag::Backup, texts);
    flag_box(ui, &mut form.settings, Flag::AskBeforeReload, texts);
    ui.add_space(8.0);
    ui.label(texts.save_backup);
    let modes = [
        (SaveBackup::None, texts.backup_none),
        (SaveBackup::Simple, texts.backup_simple),
        (SaveBackup::Verbose, texts.backup_verbose),
    ];
    for (mode, label) in modes {
        ui.radio_value(&mut form.settings.save_backup, mode, label);
    }
    ui.add_space(8.0);
    ui.label(texts.updates_title);
    let modes = [
        (UpdateMode::Off, texts.update_off),
        (UpdateMode::Notify, texts.update_notify),
        (UpdateMode::Automatic, texts.update_automatic),
    ];
    for (mode, label) in modes {
        ui.radio_value(&mut form.settings.update_mode, Some(mode), label);
    }
}

fn shortcuts_page(ui: &mut Ui, form: &mut PreferencesForm, texts: &Texts) {
    ui.add(
        TextEdit::singleline(&mut form.filter)
            .hint_text(texts.filter)
            .desired_width(240.0),
    );
    let keymap = Keymap::new(&form.settings.shortcuts);
    let needle = form.filter.to_lowercase();
    ScrollArea::vertical()
        .id_salt("cuneiedit-shortcut-list")
        .max_height(320.0)
        .show(ui, |ui| {
            egui::Grid::new("cuneiedit-shortcut-grid")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for command in shortcut_commands() {
                        let label = command.label(texts);
                        if !needle.is_empty() && !label.to_lowercase().contains(&needle) {
                            continue;
                        }
                        ui.label(label.as_ref());
                        if form.recording == Some(command) {
                            ui.strong(texts.press_keys);
                        } else {
                            let text = keymap
                                .get(command)
                                .map(|s| ui.ctx().format_shortcut(&s))
                                .unwrap_or_default();
                            ui.monospace(text);
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .small_button("✎")
                                .on_hover_text(texts.press_keys)
                                .clicked()
                            {
                                form.recording = Some(command);
                            }
                            if ui.small_button("✕").on_hover_text(texts.remove).clicked() {
                                set_change(&mut form.settings.shortcuts, command, String::new());
                            }
                            if ui.small_button("↺").on_hover_text(texts.reset).clicked() {
                                let id = command_id(command);
                                form.settings
                                    .shortcuts
                                    .retain(|(changed, _)| *changed != id);
                            }
                        });
                        ui.end_row();
                    }
                });
        });
    ui.small(texts.editor_shortcut_note);
}

fn language_fields(ui: &mut Ui, fields: &mut LanguageFields, texts: &Texts) {
    egui::Grid::new("cuneiedit-language-fields")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            let lines = [
                (texts.udl_name, &mut fields.name, false),
                (texts.udl_extensions, &mut fields.extensions, false),
                (texts.udl_keywords, &mut fields.keywords, true),
                (texts.udl_types, &mut fields.types, true),
                (texts.udl_line_comment, &mut fields.line_comment, false),
            ];
            for (label, value, multiline) in lines {
                ui.label(label);
                if multiline {
                    ui.add(
                        TextEdit::multiline(value)
                            .desired_rows(3)
                            .desired_width(360.0),
                    );
                } else {
                    ui.add(TextEdit::singleline(value).desired_width(360.0));
                }
                ui.end_row();
            }
            ui.label(texts.udl_block_comment);
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut fields.block_start).desired_width(80.0));
                ui.add(TextEdit::singleline(&mut fields.block_end).desired_width(80.0));
            });
            ui.end_row();
            ui.label(texts.udl_quotes);
            ui.add(TextEdit::singleline(&mut fields.quotes).desired_width(80.0));
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut fields.ignore_case, texts.udl_ignore_case);
            ui.end_row();
        });
    ui.small(texts.udl_words_hint);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigning_a_shortcut_takes_it_from_its_command() {
        let mut changes = Vec::new();
        let save = KeyboardShortcut::new(eframe::egui::Modifiers::COMMAND, Key::S);
        assign(&mut changes, Command::Summary, save);
        let keymap = Keymap::new(&changes);
        assert_eq!(keymap.command_for(&save), Some(Command::Summary));
        assert_eq!(keymap.get(Command::Save), None);
        assert!(shortcut_commands().contains(&Command::Summary));
    }

    #[test]
    fn converts_language_fields() {
        let fields = LanguageFields {
            name: " Mini ".into(),
            extensions: "mini mn".into(),
            keywords: "if  then\nelse".into(),
            block_start: "{-".into(),
            block_end: "".into(),
            quotes: "\" '".into(),
            ..LanguageFields::default()
        };
        let language = fields.to_language();
        assert_eq!(language.name, "Mini");
        assert_eq!(language.keywords, vec!["if", "then", "else"]);
        assert_eq!(language.block_comment, None);
        assert_eq!(language.quotes, "\"'");
        assert_eq!(LanguageFields::from(&language).extensions, "mini mn");
    }
}
