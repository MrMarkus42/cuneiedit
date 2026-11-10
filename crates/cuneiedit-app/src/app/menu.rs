//! Menu bar, tab bar and status bar.

use std::cell::Cell;

use cuneiedit_core::checksum::Algorithm;
use cuneiedit_core::highlight::Language as Syntax;
use cuneiedit_core::ops::{CommentMode, SortOrder};
use cuneiedit_core::transform::{Blank, Case};
use cuneiedit_core::{Encoding, LineEnding, Script};
use eframe::egui::{
    self, Button, Checkbox, Color32, KeyboardShortcut, MenuBar, RadioButton, ScrollArea, Sense,
    ThemePreference, Ui,
};

use super::CuneiEditApp;
use super::command::{Command, DateStyle, LineOp, PathPart};
use super::tab::{MARK_STYLES, Tab, mark_color};
use crate::i18n::{Language, Texts};
use crate::settings::Flag;
use crate::system::HAS_TRASH;
use crate::themes::THEMES;

pub fn menu_item(
    ui: &mut Ui,
    label: &str,
    shortcut: Option<KeyboardShortcut>,
    enabled: bool,
) -> bool {
    let mut button = Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
    }
    ui.add_enabled(enabled, button).clicked()
}

fn script_name(script: Script, t: &Texts) -> &'static str {
    match script {
        Script::WesternEuropean => t.script_western,
        Script::CentralEuropean => t.script_central,
        Script::Baltic => t.script_baltic,
        Script::Cyrillic => t.script_cyrillic,
        Script::Greek => t.script_greek,
        Script::Turkish => t.script_turkish,
        Script::Hebrew => t.script_hebrew,
        Script::Arabic => t.script_arabic,
        Script::NorthernEuropean => t.script_nordic,
        Script::Vietnamese => t.script_vietnamese,
        Script::Thai => t.script_thai,
        Script::Chinese => t.script_chinese,
        Script::Japanese => t.script_japanese,
        Script::Korean => t.script_korean,
        Script::Dos => t.script_dos,
    }
}

pub fn syntax_name(syntax: Syntax, texts: &Texts) -> &'static str {
    if syntax == Syntax::PLAIN_TEXT {
        texts.plain_text
    } else {
        syntax.name()
    }
}

/// How a menu item shows its state.
enum Check {
    Box(bool),
    Radio(bool),
}

const SORT_ORDERS: [SortOrder; 7] = [
    SortOrder::Lexicographic,
    SortOrder::IgnoreCase,
    SortOrder::Locale,
    SortOrder::Length,
    SortOrder::Integer,
    SortOrder::DecimalComma,
    SortOrder::DecimalDot,
];

/// Builds menus from commands; remembers the one picked.
struct Menu<'a> {
    app: &'a CuneiEditApp,
    texts: &'static Texts,
    picked: &'a Cell<Option<Command>>,
}

impl Menu<'_> {
    fn item(&self, ui: &mut Ui, command: Command) {
        let label = command.label(self.texts);
        let enabled = self.app.enabled(command);
        let clicked = match self.app.check(command) {
            Some(Check::Box(mut on)) => ui
                .add_enabled(enabled, Checkbox::new(&mut on, label.as_ref()))
                .clicked(),
            Some(Check::Radio(on)) => ui
                .add_enabled(enabled, RadioButton::new(on, label.as_ref()))
                .clicked(),
            None => menu_item(ui, &label, self.app.keymap.get(command), enabled),
        };
        if clicked {
            self.picked.set(Some(command));
        }
    }

    fn items(&self, ui: &mut Ui, commands: &[Command]) {
        for &command in commands {
            self.item(ui, command);
        }
    }

    /// Groups of items with separators between them.
    fn sections(&self, ui: &mut Ui, sections: &[&[Command]]) {
        for (index, section) in sections.iter().enumerate() {
            if index > 0 {
                ui.separator();
            }
            self.items(ui, section);
        }
    }

    fn encodings(&self, ui: &mut Ui, command: fn(Encoding) -> Command) {
        for encoding in Encoding::ALL {
            self.item(ui, command(encoding));
        }
        ui.menu_button(self.texts.character_sets, |ui| {
            for (script, encodings) in Encoding::groups() {
                ui.menu_button(script_name(script, self.texts), |ui| {
                    for encoding in encodings {
                        self.item(ui, command(encoding));
                    }
                });
            }
        });
    }
}

impl CuneiEditApp {
    fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    /// Whether a command can run now.
    pub(super) fn enabled(&self, command: Command) -> bool {
        let tab = self.active_tab();
        let has_path = tab.doc.path().is_some();
        let has_selection = !tab.editor.selection().is_empty();
        let comparing = self.compare.is_some();
        if command.edits() && tab.read_only {
            return false;
        }
        match command {
            Command::Undo => tab.doc.can_undo(),
            Command::Redo => tab.doc.can_redo(),
            Command::Cut | Command::Copy | Command::ChecksumSelection(_) => has_selection,
            Command::Reload
            | Command::Reveal
            | Command::CopyPath(_)
            | Command::CompareWithSaved
            | Command::Interpret(_)
            | Command::ToggleMonitoring => has_path,
            Command::MoveToTrash => has_path && HAS_TRASH,
            Command::CloseLeft | Command::MoveTabBackward => self.active > 0,
            Command::CloseRight | Command::MoveTabForward => self.active + 1 < self.tabs.len(),
            Command::CloseOthers | Command::Compare | Command::NextTab | Command::PreviousTab => {
                self.tabs.len() > 1
            }
            Command::RestoreClosed => !self.closed.is_empty(),
            Command::OpenAllRecent | Command::ClearRecent => !self.settings.recent.is_empty(),
            Command::Line(op) => tab.supports(op),
            Command::NextDifference
            | Command::PreviousDifference
            | Command::EndCompare
            | Command::ToggleIgnoreWhitespace
            | Command::ToggleIgnoreCase => comparing,
            Command::StartRecording => self.recording.is_none(),
            Command::StopRecording => self.recording.is_some(),
            Command::PlayMacro | Command::PlayMacroRepeatedly => {
                self.recording.is_none() && !self.macro_events.is_empty()
            }
            Command::SaveMacro => self.recording.is_none() && !self.macro_events.is_empty(),
            Command::PlaySavedMacro(_) => self.recording.is_none(),
            Command::BookmarkMatches => self.find.has_pattern(),
            Command::ClearMark(style) => tab.marks.is_set(style),
            Command::ClearAllMarks | Command::NextMark | Command::PreviousMark => tab.marks.any(),
            Command::CopyMarked => tab.marks.any(),
            Command::ToggleResults | Command::NextResult | Command::PreviousResult => {
                self.results.is_some()
            }
            Command::NextChange | Command::PreviousChange => self.settings.get(Flag::ChangeHistory),
            Command::CloseFolder => self.settings.workspace.is_some(),
            Command::MoveToOtherView => self.tabs.len() > 1 && !comparing,
            Command::CloseOtherView => self.second_view.is_some(),
            Command::MoveToNewInstance => has_path && !tab.doc.is_modified(),
            Command::FoldAll
            | Command::UnfoldAll
            | Command::FoldCurrent
            | Command::UnfoldCurrent
            | Command::HideLines
            | Command::AddCursorAbove
            | Command::AddCursorBelow
            | Command::ColumnEditor => !comparing,
            _ => true,
        }
    }

    /// Whether a toggle command is currently on.
    pub(super) fn is_on(&self, command: Command) -> bool {
        matches!(
            self.check(command),
            Some(Check::Box(true) | Check::Radio(true))
        )
    }

    fn check(&self, command: Command) -> Option<Check> {
        let tab = self.active_tab();
        let settings = &self.settings;
        let options = self.compare.as_ref().map(|view| view.state.options);
        let check = match command {
            Command::Toggle(flag) => Check::Box(settings.get(flag)),
            Command::ShowAllCharacters => Check::Box(
                settings.get(Flag::ShowWhitespace) && settings.get(Flag::ShowLineEndings),
            ),
            Command::AlwaysOnTop => Check::Box(self.always_on_top),
            Command::FullScreen => Check::Box(self.fullscreen),
            Command::DistractionFree => Check::Box(self.distraction_free),
            Command::PostIt => Check::Box(self.post_it),
            Command::SetColorTheme(index) => {
                let name = index.and_then(|i| THEMES.get(i)).map_or("", |t| t.name);
                Check::Radio(settings.color_theme == name)
            }
            Command::ToggleReadOnly => Check::Box(tab.read_only),
            Command::ToggleSplit => Check::Box(tab.split.is_some()),
            Command::ToggleMonitoring => Check::Box(tab.monitoring),
            Command::TogglePin => Check::Box(tab.pinned),
            Command::ToggleResults => Check::Box(self.show_results),
            Command::ToggleIgnoreWhitespace => {
                Check::Box(options.is_some_and(|o| o.ignore_whitespace))
            }
            Command::ToggleIgnoreCase => Check::Box(options.is_some_and(|o| o.ignore_case)),
            Command::SetTabColor(color) => Check::Radio(tab.color == color),
            Command::SetLanguage(language) => Check::Radio(settings.language == language),
            Command::SetTheme(theme) => Check::Radio(settings.theme == theme),
            Command::SetTabWidth(width) => Check::Radio(settings.tab_width == width),
            Command::SetLineEnding(ending) => Check::Radio(tab.doc.line_ending() == ending),
            Command::Convert(encoding) => Check::Radio(tab.doc.encoding() == encoding),
            _ => return None,
        };
        Some(check)
    }

    pub(super) fn menu_bar(&self, ui: &mut Ui) -> Option<Command> {
        let t = self.texts();
        let picked = Cell::new(None);
        let m = Menu {
            app: self,
            texts: t,
            picked: &picked,
        };
        let tab = self.active_tab();
        MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t.menu_file, |ui| self.file_menu(ui, &m));
            ui.menu_button(t.menu_edit, |ui| edit_menu(ui, &m));
            ui.menu_button(t.menu_search, |ui| search_menu(ui, &m));
            ui.menu_button(t.menu_view, |ui| view_menu(ui, &m));
            ui.menu_button(t.menu_format, |ui| {
                ui.menu_button(t.interpret_as, |ui| m.encodings(ui, Command::Interpret));
                ui.menu_button(t.convert_to, |ui| m.encodings(ui, Command::Convert));
                ui.separator();
                ui.menu_button(t.line_ending, |ui| {
                    for ending in LineEnding::ALL {
                        m.item(ui, Command::SetLineEnding(ending));
                    }
                });
            });
            ui.menu_button(t.menu_syntax, |ui| {
                let pick = |ui: &mut Ui, syntax: Syntax| {
                    if ui
                        .radio(tab.syntax() == syntax, syntax_name(syntax, t))
                        .clicked()
                    {
                        picked.set(Some(Command::SetSyntax(syntax)));
                    }
                };
                pick(ui, Syntax::PLAIN_TEXT);
                ui.separator();
                let first_letter = |syntax: &Syntax| syntax.name().chars().next();
                let mut built_in: Vec<Syntax> = Syntax::all()
                    .filter(|s| *s != Syntax::PLAIN_TEXT && !s.is_user_defined())
                    .collect();
                built_in.sort_by_key(|s| s.name().to_lowercase());
                for group in built_in.chunk_by(|a, b| first_letter(a) == first_letter(b)) {
                    let letter: String = first_letter(&group[0]).into_iter().collect();
                    ui.menu_button(letter, |ui| {
                        for &syntax in group {
                            pick(ui, syntax);
                        }
                    });
                }
                let user: Vec<Syntax> = Syntax::all().filter(|s| s.is_user_defined()).collect();
                if !user.is_empty() {
                    ui.separator();
                    ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for syntax in user {
                            pick(ui, syntax);
                        }
                    });
                }
                ui.separator();
                m.item(ui, Command::UserLanguages);
            });
            ui.menu_button(t.menu_compare, |ui| {
                m.sections(
                    ui,
                    &[
                        &[
                            Command::SetCompareFirst,
                            Command::Compare,
                            Command::CompareWithFile,
                            Command::CompareWithSaved,
                        ],
                        &[Command::NextDifference, Command::PreviousDifference],
                        &[Command::ToggleIgnoreWhitespace, Command::ToggleIgnoreCase],
                        &[Command::EndCompare],
                    ],
                );
            });
            ui.menu_button(t.menu_settings, |ui| {
                ui.menu_button(t.language, |ui| {
                    for language in Language::ALL {
                        m.item(ui, Command::SetLanguage(language));
                    }
                });
                ui.menu_button(t.theme, |ui| {
                    for theme in [
                        ThemePreference::System,
                        ThemePreference::Light,
                        ThemePreference::Dark,
                    ] {
                        m.item(ui, Command::SetTheme(theme));
                    }
                });
                ui.menu_button(t.color_theme, |ui| {
                    m.item(ui, Command::SetColorTheme(None));
                    for index in 0..THEMES.len() {
                        m.item(ui, Command::SetColorTheme(Some(index)));
                    }
                });
                ui.menu_button(t.tab_width, |ui| {
                    for width in [2, 3, 4, 8] {
                        m.item(ui, Command::SetTabWidth(width));
                    }
                });
                m.sections(
                    ui,
                    &[
                        &[
                            Command::Toggle(Flag::IndentWithSpaces),
                            Command::Toggle(Flag::AutoClose),
                            Command::Toggle(Flag::AutoComplete),
                        ],
                        &[
                            Command::Toggle(Flag::Backup),
                            Command::Toggle(Flag::AskBeforeReload),
                        ],
                        &[Command::Preferences],
                    ],
                );
            });
            ui.menu_button(t.menu_macro, |ui| {
                m.items(
                    ui,
                    &[
                        Command::StartRecording,
                        Command::StopRecording,
                        Command::PlayMacro,
                        Command::PlayMacroRepeatedly,
                    ],
                );
                ui.separator();
                m.item(ui, Command::SaveMacro);
                if !self.saved_macros.is_empty() {
                    ui.separator();
                    let enabled = self.enabled(Command::PlaySavedMacro(0));
                    for (index, saved) in self.saved_macros.iter().enumerate() {
                        if menu_item(ui, &saved.name, None, enabled) {
                            picked.set(Some(Command::PlaySavedMacro(index)));
                        }
                    }
                    ui.menu_button(t.delete_macro, |ui| {
                        for (index, saved) in self.saved_macros.iter().enumerate() {
                            if ui.button(&saved.name).clicked() {
                                picked.set(Some(Command::DeleteMacro(index)));
                            }
                        }
                    });
                }
            });
            ui.menu_button(t.menu_tools, |ui| {
                for algorithm in Algorithm::ALL {
                    ui.menu_button(algorithm.label(), |ui| {
                        m.items(
                            ui,
                            &[
                                Command::ChecksumFiles(algorithm),
                                Command::ChecksumSelection(algorithm),
                            ],
                        );
                    });
                }
            });
            ui.menu_button(t.menu_help, |ui| {
                m.item(ui, Command::CheckForUpdates);
                m.item(ui, Command::About);
            });
        });
        picked.get()
    }

    fn file_menu(&self, ui: &mut Ui, m: &Menu<'_>) {
        let t = m.texts;
        m.items(
            ui,
            &[
                Command::New,
                Command::Open,
                Command::Reveal,
                Command::Reload,
            ],
        );
        ui.menu_button(t.recent_files, |ui| {
            for (index, path) in self.settings.recent.iter().enumerate() {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                if ui
                    .button(name)
                    .on_hover_text(path.display().to_string())
                    .clicked()
                {
                    m.picked.set(Some(Command::OpenRecent(index)));
                }
            }
            if !self.settings.recent.is_empty() {
                ui.separator();
            }
            m.items(ui, &[Command::OpenAllRecent, Command::ClearRecent]);
        });
        m.item(ui, Command::RestoreClosed);
        ui.separator();
        m.items(
            ui,
            &[
                Command::Save,
                Command::SaveAs,
                Command::SaveCopy,
                Command::SaveAll,
                Command::Rename,
            ],
        );
        ui.separator();
        m.items(ui, &[Command::Close, Command::CloseAll]);
        ui.menu_button(t.close_more, |ui| {
            m.items(
                ui,
                &[
                    Command::CloseOthers,
                    Command::CloseLeft,
                    Command::CloseRight,
                    Command::CloseUnchanged,
                ],
            );
        });
        m.sections(
            ui,
            &[
                &[Command::MoveToTrash],
                &[Command::OpenFolder, Command::CloseFolder],
                &[Command::LoadSession, Command::SaveSession],
                &[Command::Print, Command::ExportPdf, Command::ExportHtml],
                &[Command::Quit],
            ],
        );
    }

    /// Commands from clicks on tabs. A context menu command comes after activating its tab.
    pub(super) fn tab_bar(&self, ui: &mut Ui) -> Vec<Command> {
        let texts = self.texts();
        let mut commands = Vec::new();
        ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for (index, tab) in self.tabs.iter().enumerate() {
                    let mut title = tab.title(texts);
                    if tab.pinned {
                        title.insert_str(0, "📌 ");
                    }
                    if tab.read_only {
                        title.push_str(" 🔒");
                    }
                    if tab.doc.is_modified() {
                        title.push_str(" ●");
                    }
                    let response = ui
                        .selectable_label(index == self.active, title)
                        .interact(Sense::drag());
                    response.dnd_set_drag_payload(index);
                    if let Some(from) = response.dnd_release_payload::<usize>() {
                        commands.push(Command::MoveTab(*from, index));
                    } else if response.dnd_hover_payload::<usize>().is_some() {
                        let rect = response.rect;
                        ui.painter().vline(
                            rect.left() - 2.0,
                            rect.y_range(),
                            egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                        );
                    }
                    if let Some(color) = tab.color {
                        let rect = response.rect;
                        ui.painter().hline(
                            rect.x_range(),
                            rect.bottom(),
                            egui::Stroke::new(3.0, solid(mark_color(color))),
                        );
                    }
                    if let Some(path) = tab.doc.path() {
                        response.clone().on_hover_text(path.display().to_string());
                    }
                    if response.clicked() {
                        commands.push(Command::Activate(index));
                    }
                    if response.middle_clicked() || (!tab.pinned && ui.small_button("×").clicked())
                    {
                        commands.push(Command::CloseTab(index));
                    }
                    response.context_menu(|ui| {
                        if let Some(command) = self.tab_context_menu(ui) {
                            commands.push(Command::Activate(index));
                            commands.push(command);
                        }
                    });
                    ui.separator();
                }
            });
        });
        commands
    }

    /// Commands that act on the active tab, shown in a tab's context menu.
    fn tab_context_menu(&self, ui: &mut Ui) -> Option<Command> {
        let t = self.texts();
        let picked = Cell::new(None);
        let m = Menu {
            app: self,
            texts: t,
            picked: &picked,
        };
        m.sections(
            ui,
            &[
                &[
                    Command::Close,
                    Command::CloseOthers,
                    Command::CloseLeft,
                    Command::CloseRight,
                    Command::CloseUnchanged,
                ],
                &[
                    Command::Save,
                    Command::SaveAs,
                    Command::Rename,
                    Command::Reload,
                ],
                &[
                    Command::Reveal,
                    Command::CopyPath(PathPart::Full),
                    Command::CopyPath(PathPart::Name),
                    Command::CopyPath(PathPart::Directory),
                ],
                &[Command::ToggleReadOnly, Command::TogglePin],
                &[Command::MoveToOtherView, Command::MoveToNewInstance],
            ],
        );
        ui.menu_button(t.tab_color, |ui| tab_colors(ui, &m));
        ui.separator();
        m.item(ui, Command::MoveToTrash);
        picked.get()
    }

    pub(super) fn status_bar(&self, ui: &mut Ui) {
        let texts = self.texts();
        let tab = self.active_tab();
        let text = tab.doc.text();
        let (line, column) = tab.editor.line_col(&tab.doc);
        let selection = tab.editor.selection().len();
        ui.horizontal(|ui| {
            ui.label(syntax_name(tab.syntax(), texts));
            ui.separator();
            ui.label(format!(
                "{} {}    {} {}",
                texts.status_length,
                text.len_chars(),
                texts.status_lines,
                text.len_lines()
            ));
            if self.recording.is_some() {
                ui.separator();
                ui.colored_label(
                    Color32::from_rgb(220, 50, 50),
                    format!("● {}", texts.recording),
                );
            }
            if tab.editor.has_multiple_selections() {
                ui.separator();
                let count = tab.editor.selections().len();
                ui.label(format!("{count} {}", texts.status_cursors));
            }
            if tab.read_only {
                ui.separator();
                ui.label(texts.read_only);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(tab.doc.encoding().label());
                ui.separator();
                ui.label(tab.doc.line_ending().label());
                ui.separator();
                ui.label(format!(
                    "{} {}, {} {}, {} {}",
                    texts.status_line,
                    line + 1,
                    texts.status_column,
                    column + 1,
                    texts.status_selection,
                    selection
                ));
            });
        });
    }
}

fn solid(color: Color32) -> Color32 {
    Color32::from_rgb(color.r(), color.g(), color.b())
}

fn tab_colors(ui: &mut Ui, m: &Menu<'_>) {
    m.item(ui, Command::SetTabColor(None));
    for color in 0..MARK_STYLES {
        m.item(ui, Command::SetTabColor(Some(color)));
    }
}

fn edit_menu(ui: &mut Ui, m: &Menu<'_>) {
    let t = m.texts;
    m.sections(
        ui,
        &[
            &[Command::Undo, Command::Redo],
            &[
                Command::Cut,
                Command::Copy,
                Command::Paste,
                Command::SelectAll,
            ],
        ],
    );
    ui.separator();
    ui.menu_button(t.insert_date, |ui| {
        m.items(
            ui,
            &[
                Command::InsertDate(DateStyle::Short),
                Command::InsertDate(DateStyle::Long),
                Command::InsertDate(DateStyle::Custom),
            ],
        );
    });
    ui.menu_button(t.copy_to_clipboard, |ui| {
        m.items(
            ui,
            &[
                Command::CopyPath(PathPart::Full),
                Command::CopyPath(PathPart::Name),
                Command::CopyPath(PathPart::Directory),
            ],
        );
    });
    ui.menu_button(t.indent, |ui| {
        m.items(
            ui,
            &[
                Command::Line(LineOp::Indent),
                Command::Line(LineOp::Unindent),
            ],
        );
    });
    ui.menu_button(t.convert_case, |ui| {
        let cases = [
            Case::Upper,
            Case::Lower,
            Case::Proper,
            Case::ProperBlend,
            Case::Sentence,
            Case::SentenceBlend,
            Case::Invert,
            Case::Random(0),
        ];
        for case in cases {
            m.item(ui, Command::Line(LineOp::Case(case)));
        }
    });
    ui.menu_button(t.line_operations, |ui| {
        let line = Command::Line;
        m.sections(
            ui,
            &[
                &[
                    line(LineOp::Duplicate),
                    line(LineOp::Delete),
                    line(LineOp::MoveUp),
                    line(LineOp::MoveDown),
                ],
                &[
                    line(LineOp::RemoveDuplicates),
                    line(LineOp::RemoveConsecutiveDuplicates),
                ],
                &[line(LineOp::Split), line(LineOp::Join)],
                &[line(LineOp::BlankAbove), line(LineOp::BlankBelow)],
                &[line(LineOp::RemoveEmpty), line(LineOp::RemoveBlank)],
                &[line(LineOp::Reverse), line(LineOp::Shuffle)],
            ],
        );
        ui.separator();
        ui.menu_button(t.sort_lines, |ui| {
            for (index, order) in SORT_ORDERS.into_iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                m.items(
                    ui,
                    &[
                        Command::Line(LineOp::Sort(order, false)),
                        Command::Line(LineOp::Sort(order, true)),
                    ],
                );
            }
        });
    });
    ui.menu_button(t.comment_menu, |ui| {
        m.items(
            ui,
            &[
                Command::Line(LineOp::Comment(CommentMode::Toggle)),
                Command::Line(LineOp::Comment(CommentMode::Comment)),
                Command::Line(LineOp::Comment(CommentMode::Uncomment)),
                Command::Line(LineOp::BlockComment(true)),
                Command::Line(LineOp::BlockComment(false)),
            ],
        );
    });
    ui.menu_button(t.blank_operations, |ui| {
        let blank = |b| Command::Line(LineOp::Blank(b));
        m.sections(
            ui,
            &[
                &[
                    Command::Line(LineOp::TrimTrailing),
                    blank(Blank::TrimLeading),
                    blank(Blank::TrimBoth),
                    blank(Blank::EolToSpace),
                    blank(Blank::TrimAndEolToSpace),
                ],
                &[
                    blank(Blank::TabsToSpaces),
                    blank(Blank::SpacesToTabs(false)),
                    blank(Blank::SpacesToTabs(true)),
                ],
            ],
        );
    });
    m.sections(
        ui,
        &[
            &[Command::Complete],
            &[Command::Line(LineOp::Redact), Command::ToggleReadOnly],
        ],
    );
    ui.separator();
    ui.menu_button(t.multi_editing, |ui| {
        m.items(
            ui,
            &[
                Command::AddCursorAbove,
                Command::AddCursorBelow,
                Command::ColumnEditor,
            ],
        );
        ui.separator();
        ui.weak(t.multi_editing_hint);
    });
}

fn search_menu(ui: &mut Ui, m: &Menu<'_>) {
    let t = m.texts;
    m.sections(
        ui,
        &[
            &[
                Command::OpenFind,
                Command::FindInFiles,
                Command::FindNext,
                Command::FindPrevious,
                Command::SelectFindNext,
                Command::SelectFindPrevious,
            ],
            &[Command::OpenReplace],
            &[
                Command::FindAll,
                Command::FindAllInTabs,
                Command::ToggleResults,
                Command::NextResult,
                Command::PreviousResult,
            ],
            &[
                Command::GoTo,
                Command::MatchingBrace,
                Command::SelectBetweenBraces,
            ],
        ],
    );
    ui.separator();
    ui.menu_button(t.mark_menu, |ui| {
        for style in 0..MARK_STYLES {
            m.item(ui, Command::Mark(style));
        }
    });
    ui.menu_button(t.clear_style_menu, |ui| {
        for style in 0..MARK_STYLES {
            m.item(ui, Command::ClearMark(style));
        }
        ui.separator();
        m.item(ui, Command::ClearAllMarks);
    });
    m.items(
        ui,
        &[
            Command::NextMark,
            Command::PreviousMark,
            Command::CopyMarked,
        ],
    );
    ui.menu_button(t.bookmarks, |ui| {
        m.sections(
            ui,
            &[
                &[
                    Command::ToggleBookmark,
                    Command::NextBookmark,
                    Command::PreviousBookmark,
                    Command::ClearBookmarks,
                ],
                &[
                    Command::CutBookmarked,
                    Command::CopyBookmarked,
                    Command::PasteToBookmarked,
                    Command::RemoveBookmarked,
                    Command::RemoveUnbookmarked,
                    Command::InvertBookmarks,
                ],
                &[Command::BookmarkMatches],
            ],
        );
    });
    ui.separator();
    m.items(
        ui,
        &[
            Command::FindCharsInRange,
            Command::NextChange,
            Command::PreviousChange,
        ],
    );
}

fn view_menu(ui: &mut Ui, m: &Menu<'_>) {
    let t = m.texts;
    m.items(
        ui,
        &[
            Command::AlwaysOnTop,
            Command::FullScreen,
            Command::DistractionFree,
            Command::PostIt,
            Command::Toggle(Flag::Toolbar),
        ],
    );
    ui.separator();
    ui.menu_button(t.show_symbol, |ui| {
        m.sections(
            ui,
            &[
                &[
                    Command::Toggle(Flag::ShowWhitespace),
                    Command::Toggle(Flag::ShowLineEndings),
                    Command::ShowAllCharacters,
                ],
                &[
                    Command::Toggle(Flag::IndentGuides),
                    Command::Toggle(Flag::ShowWrapSymbol),
                    Command::Toggle(Flag::EdgeLine),
                ],
            ],
        );
    });
    m.sections(
        ui,
        &[
            &[Command::ZoomIn, Command::ZoomOut, Command::ZoomReset],
            &[
                Command::Toggle(Flag::WordWrap),
                Command::Toggle(Flag::ChangeHistory),
                Command::Toggle(Flag::SmartHighlight),
            ],
        ],
    );
    ui.menu_button(t.folding_menu, |ui| {
        m.sections(
            ui,
            &[
                &[
                    Command::FoldAll,
                    Command::UnfoldAll,
                    Command::FoldCurrent,
                    Command::UnfoldCurrent,
                ],
                &[Command::HideLines],
                &[Command::Toggle(Flag::Folding)],
            ],
        );
    });
    ui.separator();
    ui.menu_button(t.tab_menu, |ui| {
        m.sections(
            ui,
            &[
                &[
                    Command::NextTab,
                    Command::PreviousTab,
                    Command::MoveTabForward,
                    Command::MoveTabBackward,
                ],
                &[Command::TogglePin],
            ],
        );
        ui.menu_button(t.tab_color, |ui| tab_colors(ui, m));
    });
    m.items(
        ui,
        &[
            Command::ToggleSplit,
            Command::MoveToOtherView,
            Command::CloseOtherView,
            Command::MoveToNewInstance,
            Command::ToggleMonitoring,
        ],
    );
    ui.separator();
    ui.menu_button(t.panels, |ui| {
        m.items(
            ui,
            &[
                Command::Toggle(Flag::FileTree),
                Command::Toggle(Flag::DocumentMap),
                Command::Toggle(Flag::FunctionList),
                Command::Toggle(Flag::DocumentList),
                Command::Toggle(Flag::ClipboardHistory),
                Command::Toggle(Flag::CharacterPanel),
                Command::ToggleResults,
            ],
        );
    });
    m.item(ui, Command::Summary);
}
