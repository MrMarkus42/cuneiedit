//! What each command does.

use std::path::Path;

use cuneiedit_core::checksum::{self, Algorithm};
use cuneiedit_core::code::hits_by_line;
use cuneiedit_core::ops;
use cuneiedit_core::transform::{enclosing_brackets, matching_bracket, summary};
use cuneiedit_editor::Repeat;
use eframe::egui::{self, Event, ViewportCommand, WindowLevel};

use super::CuneiEditApp;
use super::command::{Command, DateStyle, PathPart};
use super::dialogs::{CharPreset, ColumnForm, Dialog};
use super::panels::{Results, Source};
use super::tab::Split;
use crate::find::FindAction;
use crate::i18n::fill;
use crate::macros;
use crate::settings::{DEFAULT_FONT_SIZE, Flag};
use crate::themes::THEMES;

/// The current local time in a strftime format, or `None` if the format is invalid.
pub fn format_now(format: &str) -> Option<String> {
    jiff::fmt::strtime::format(format, &jiff::Zoned::now()).ok()
}

/// The selection, or the word at the cursor if nothing is selected.
fn selection_or_word(tab: &super::Tab) -> String {
    let selection = tab.editor.selection();
    if !selection.is_empty() {
        return tab.doc.slice(selection);
    }
    let rope = tab.doc.text();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let cursor = tab.editor.cursor().min(rope.len_chars());
    let mut start = cursor;
    while start > 0 && is_word(rope.char(start - 1)) {
        start -= 1;
    }
    let mut end = cursor;
    while end < rope.len_chars() && is_word(rope.char(end)) {
        end += 1;
    }
    tab.doc.slice(start..end)
}

impl CuneiEditApp {
    pub(super) fn run(&mut self, ctx: &egui::Context, command: Command) {
        if command.edits() && self.tabs[self.active].read_only {
            return;
        }
        if self.run_global(ctx, command) {
            return;
        }
        self.run_on_tab(ctx, command);
        // These move keyboard focus to their own text field.
        let keeps_focus = matches!(command, Command::OpenFind | Command::OpenReplace)
            || (self.find.open && matches!(command, Command::FindNext | Command::FindPrevious));
        if !keeps_focus && self.dialog.is_none() {
            self.tab().editor.request_focus();
        }
    }

    /// Commands about files, windows and settings. Returns whether `command` was one.
    fn run_global(&mut self, ctx: &egui::Context, command: Command) -> bool {
        let id = self.tabs[self.active].id;
        match command {
            Command::New => self.new_tab(),
            Command::Open => self.open_dialog(),
            Command::OpenRecent(index) => {
                if let Some(path) = self.settings.recent.get(index).cloned() {
                    self.open_path(&path);
                }
            }
            Command::OpenAllRecent => {
                for path in self.settings.recent.clone().iter().rev() {
                    self.open_path(path);
                }
            }
            Command::ClearRecent => {
                self.settings.recent.clear();
                self.persist();
            }
            Command::RestoreClosed => {
                if let Some(path) = self.closed.pop() {
                    self.open_path(&path);
                }
            }
            Command::Reload => self.reload_tab(self.active, None),
            Command::Interpret(encoding) => self.reload_tab(self.active, Some(encoding)),
            Command::Save => {
                self.save(self.active);
            }
            Command::SaveAs => {
                self.save_as(self.active);
            }
            Command::SaveCopy => self.save_copy(),
            Command::SaveAll => self.save_all(),
            Command::Rename => {
                let tab = &self.tabs[self.active];
                if tab.doc.path().is_some() {
                    self.dialog = Some(Dialog::Rename {
                        tab: id,
                        name: tab.doc.file_name().unwrap_or_default(),
                        focus: true,
                    });
                } else {
                    self.save_as(self.active);
                }
            }
            Command::Close => self.close_tabs(ctx, vec![id]),
            Command::CloseTab(index) => {
                if let Some(tab) = self.tabs.get(index) {
                    self.close_tabs(ctx, vec![tab.id]);
                }
            }
            Command::CloseAll
            | Command::CloseOthers
            | Command::CloseLeft
            | Command::CloseRight
            | Command::CloseUnchanged => {
                let active = self.active;
                let ids = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(index, tab)| {
                        !tab.pinned
                            && match command {
                                Command::CloseOthers => *index != active,
                                Command::CloseLeft => *index < active,
                                Command::CloseRight => *index > active,
                                Command::CloseUnchanged => !tab.doc.is_modified(),
                                _ => true,
                            }
                    })
                    .map(|(_, tab)| tab.id)
                    .collect();
                self.close_tabs(ctx, ids);
            }
            Command::MoveToTrash => self.dialog = Some(Dialog::ConfirmTrash { tab: id }),
            Command::Reveal => self.reveal(),
            Command::LoadSession => self.load_session_file(),
            Command::SaveSession => self.save_session_file(),
            Command::OpenFolder => {
                let mut dialog = rfd::FileDialog::new();
                if let Some(dir) = self
                    .settings
                    .workspace
                    .clone()
                    .or_else(|| self.dialog_dir())
                {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(dir) = dialog.pick_folder() {
                    self.settings.workspace = Some(dir);
                    self.settings.set(Flag::FileTree, true);
                    self.tree.refresh();
                    self.persist();
                }
            }
            Command::CloseFolder => {
                self.settings.workspace = None;
                self.settings.set(Flag::FileTree, false);
                self.tree.refresh();
                self.persist();
            }
            Command::ExportHtml | Command::ExportPdf => {
                self.export(ctx, command == Command::ExportPdf);
            }
            Command::Print => self.print(ctx),
            Command::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            Command::Activate(index) => self.activate(index),
            Command::NextTab | Command::PreviousTab => {
                let len = self.tabs.len();
                let step = if command == Command::NextTab {
                    1
                } else {
                    len - 1
                };
                self.activate((self.active + step) % len);
            }
            Command::MoveTabForward | Command::MoveTabBackward => {
                let target = if command == Command::MoveTabForward {
                    self.active + 1
                } else {
                    self.active.wrapping_sub(1)
                };
                if target < self.tabs.len() {
                    self.tabs.swap(self.active, target);
                    self.active = target;
                }
            }
            Command::MoveTab(from, to) => {
                if from < self.tabs.len() && to < self.tabs.len() && from != to {
                    let tab = self.tabs.remove(from);
                    self.tabs.insert(to, tab);
                    self.active = self.tab_index(id).unwrap_or(0);
                }
            }
            Command::MoveToOtherView => {
                if self.tabs.len() > 1 {
                    self.second_view = Some(id);
                    let neighbour = if self.active > 0 { self.active - 1 } else { 1 };
                    self.activate(neighbour);
                }
            }
            Command::CloseOtherView => self.second_view = None,
            Command::MoveToNewInstance => self.move_to_new_instance(),
            Command::DistractionFree => {
                self.distraction_free ^= true;
                self.post_it = false;
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.distraction_free));
            }
            Command::PostIt => {
                self.post_it ^= true;
                if self.distraction_free {
                    self.distraction_free = false;
                    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
                }
                let level = if self.post_it || self.always_on_top {
                    WindowLevel::AlwaysOnTop
                } else {
                    WindowLevel::Normal
                };
                ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
            }
            Command::NextResult | Command::PreviousResult => {
                self.step_result(command == Command::NextResult);
            }
            Command::TogglePin => {
                let mut tab = self.tabs.remove(self.active);
                tab.pinned ^= true;
                // Pinned tabs stay together at the front.
                let position = self.tabs.iter().take_while(|tab| tab.pinned).count();
                self.tabs.insert(position, tab);
                self.active = position;
            }
            Command::Toggle(flag) => {
                self.settings.toggle(flag);
                self.persist();
            }
            Command::ShowAllCharacters => {
                let on = !(self.settings.get(Flag::ShowWhitespace)
                    && self.settings.get(Flag::ShowLineEndings));
                self.settings.set(Flag::ShowWhitespace, on);
                self.settings.set(Flag::ShowLineEndings, on);
                self.persist();
            }
            Command::AlwaysOnTop => {
                self.always_on_top ^= true;
                let level = if self.always_on_top {
                    WindowLevel::AlwaysOnTop
                } else {
                    WindowLevel::Normal
                };
                ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
            }
            Command::FullScreen => {
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!self.fullscreen));
            }
            Command::ZoomIn | Command::ZoomOut | Command::ZoomReset => {
                let size = match command {
                    Command::ZoomIn => self.settings.font_size + 1.0,
                    Command::ZoomOut => self.settings.font_size - 1.0,
                    _ => DEFAULT_FONT_SIZE,
                };
                self.settings.set_font_size(size);
                self.persist();
            }
            Command::SetLanguage(language) => {
                self.settings.language = language;
                self.persist();
            }
            Command::SetTheme(theme) => {
                self.settings.theme = theme;
                ctx.set_theme(theme);
                self.persist();
            }
            Command::SetTabWidth(width) => {
                self.settings.tab_width = width;
                self.persist();
            }
            Command::SetColorTheme(theme) => {
                self.settings.color_theme = theme
                    .and_then(|index| THEMES.get(index))
                    .map(|theme| theme.name.to_owned())
                    .unwrap_or_default();
                self.persist();
            }
            Command::Preferences => self.open_preferences(),
            Command::UserLanguages => self.open_user_languages(),
            Command::CheckForUpdates => self.check_for_updates(ctx),
            Command::About => {
                let texts = self.texts();
                self.dialog = Some(Dialog::Message {
                    title: texts.about.to_owned(),
                    text: format!(
                        "CuneiEdit {}\n\n{}",
                        env!("CARGO_PKG_VERSION"),
                        texts.about_text
                    ),
                });
            }
            Command::FindInFiles => {
                if self.files_form.dir.trim().is_empty() {
                    let dir = self.dialog_dir().or_else(|| std::env::current_dir().ok());
                    self.files_form.dir = dir.map(|d| d.display().to_string()).unwrap_or_default();
                }
                let selected = self.tabs[self.active].editor.selection();
                let tab = &self.tabs[self.active];
                let text = tab.doc.slice(selected);
                if !text.is_empty() && !text.contains('\n') {
                    self.find.query_mut().pattern = text;
                }
                self.dialog = Some(Dialog::FindInFiles { focus: true });
            }
            Command::ToggleResults => self.show_results ^= true,
            Command::FindAll => self.find_all(false),
            Command::FindAllInTabs => self.find_all(true),
            Command::SetCompareFirst => self.compare_first = Some(id),
            Command::Compare => self.compare_with_neighbor(),
            Command::CompareWithFile => self.compare_with_file(),
            Command::CompareWithSaved => self.compare_with_saved(),
            Command::EndCompare => self.compare = None,
            Command::NextDifference | Command::PreviousDifference => {
                self.go_to_difference(command == Command::NextDifference);
            }
            Command::ToggleIgnoreWhitespace | Command::ToggleIgnoreCase => {
                if let Some(view) = &mut self.compare {
                    let options = &mut view.state.options;
                    if command == Command::ToggleIgnoreCase {
                        options.ignore_case ^= true;
                    } else {
                        options.ignore_whitespace ^= true;
                    }
                }
            }
            Command::StartRecording => {
                self.tab().editor.start_recording();
                self.recording = Some(id);
            }
            Command::StopRecording => {
                if let Some(index) = self.recording.take().and_then(|id| self.tab_index(id)) {
                    self.macro_events = self.tabs[index].editor.stop_recording();
                }
            }
            Command::PlayMacroRepeatedly => {
                self.dialog = Some(Dialog::PlayMacro {
                    times: "2".into(),
                    until_end: false,
                });
            }
            Command::SaveMacro => {
                self.dialog = Some(Dialog::SaveMacro {
                    name: String::new(),
                    focus: true,
                });
            }
            Command::DeleteMacro(index) => {
                if index < self.saved_macros.len() {
                    self.saved_macros.remove(index);
                    if let Err(err) = macros::save(&self.saved_macros) {
                        self.error(err.to_string());
                    }
                }
            }
            Command::ChecksumFiles(algorithm) => self.checksum_files(algorithm),
            _ => return false,
        }
        true
    }

    /// Commands on the active document.
    fn run_on_tab(&mut self, ctx: &egui::Context, command: Command) {
        let texts = self.texts();
        let settings = &self.settings;
        let tab = &mut self.tabs[self.active];
        let id = tab.id;
        match command {
            Command::Undo => tab.editor.undo(&mut tab.doc),
            Command::Redo => tab.editor.redo(&mut tab.doc),
            Command::Cut => tab.editor.cut(&mut tab.doc, ctx),
            Command::Copy => tab.editor.copy(&tab.doc, ctx),
            Command::Paste => ctx.send_viewport_cmd(ViewportCommand::RequestPaste),
            Command::SelectAll => tab.editor.select_all(&tab.doc),
            Command::InsertDate(style) => {
                let format = match style {
                    DateStyle::Short => "%Y-%m-%d %H:%M",
                    DateStyle::Long => "%A, %d %B %Y %H:%M:%S",
                    DateStyle::Custom => &settings.date_format,
                };
                match format_now(format) {
                    Some(text) => tab.editor.insert(&mut tab.doc, &text),
                    None => self.error(texts.invalid_date_format.to_owned()),
                }
            }
            Command::CopyPath(part) => {
                let Some(path) = tab.doc.path() else {
                    return;
                };
                let text = match part {
                    PathPart::Full => Some(path),
                    PathPart::Name => path.file_name().map(Path::new),
                    PathPart::Directory => path.parent(),
                };
                if let Some(text) = text {
                    ctx.copy_text(text.display().to_string());
                }
            }
            Command::Line(op) => {
                if let Err(err) = tab.apply_line_op(op, settings) {
                    let line = (err.line + 1).to_string();
                    self.error(fill(texts.sort_failed, &[("line", &line)]));
                }
            }
            Command::ToggleReadOnly => tab.read_only ^= true,
            Command::Complete => self.start_completion(1, false),
            Command::AddCursorAbove | Command::AddCursorBelow => {
                let down = command == Command::AddCursorBelow;
                tab.editor
                    .add_cursor_vertically(&tab.doc, down, settings.tab_width);
            }
            Command::ColumnEditor => {
                self.dialog = Some(Dialog::ColumnEditor(ColumnForm::default()))
            }
            Command::NextMark | Command::PreviousMark => {
                let selection = tab.editor.selection();
                let spans = tab.marks.spans(&tab.doc);
                let target = if command == Command::NextMark {
                    spans
                        .iter()
                        .find(|(r, _)| r.start >= selection.end && *r != selection)
                        .or(spans.first())
                } else {
                    spans
                        .iter()
                        .rev()
                        .find(|(r, _)| r.end <= selection.start && *r != selection)
                        .or(spans.last())
                };
                if let Some((range, _)) = target.cloned() {
                    tab.editor.set_selection(range.start, range.end);
                }
            }
            Command::CopyMarked => {
                let texts: Vec<String> = tab
                    .marks
                    .spans(&tab.doc)
                    .iter()
                    .map(|(range, _)| tab.doc.slice(range.clone()))
                    .collect();
                if !texts.is_empty() {
                    ctx.copy_text(texts.join("\n"));
                }
            }
            Command::NextChange | Command::PreviousChange => {
                let mut starts: Vec<usize> = tab
                    .history
                    .marks(&tab.doc)
                    .iter()
                    .map(|(r, _)| r.start)
                    .collect();
                starts.sort_unstable();
                starts.dedup();
                let (line, _) = tab.editor.line_col(&tab.doc);
                let target = if command == Command::NextChange {
                    starts.iter().find(|&&l| l > line).or(starts.first())
                } else {
                    starts.iter().rev().find(|&&l| l < line).or(starts.last())
                };
                if let Some(&line) = target {
                    let rope = tab.doc.text();
                    tab.editor
                        .set_cursor(rope.line_to_char(line.min(rope.len_lines() - 1)));
                }
            }
            Command::FoldAll => tab.editor.fold_all(&tab.doc, settings.tab_width),
            Command::UnfoldAll => tab.editor.unfold_all(),
            Command::FoldCurrent | Command::UnfoldCurrent => {
                let fold = command == Command::FoldCurrent;
                tab.editor.fold_current(&tab.doc, settings.tab_width, fold);
            }
            Command::HideLines => tab.editor.hide_lines(&tab.doc),
            Command::OpenFind | Command::OpenReplace => {
                let replace = command == Command::OpenReplace;
                self.find.show(replace, &tab.editor.selected_text(&tab.doc));
            }
            Command::FindNext | Command::FindPrevious if !self.find.has_pattern() => {
                self.find.show(false, &tab.editor.selected_text(&tab.doc));
            }
            Command::FindNext => self.find_action(FindAction::Next),
            Command::FindPrevious => self.find_action(FindAction::Previous),
            Command::SelectFindNext | Command::SelectFindPrevious => {
                let text = selection_or_word(tab);
                let forward = command == Command::SelectFindNext;
                self.find
                    .select_and_find(&text, forward, id, &tab.doc, &mut tab.editor);
            }
            Command::GoTo => {
                self.dialog = Some(Dialog::GoTo {
                    input: String::new(),
                    offset: false,
                    focus: true,
                });
            }
            Command::MatchingBrace => {
                if let Some((_, other)) = matching_bracket(tab.doc.text(), tab.editor.cursor()) {
                    tab.editor.set_cursor(other);
                }
            }
            Command::SelectBetweenBraces => {
                if let Some(inner) = enclosing_brackets(tab.doc.text(), tab.editor.cursor()) {
                    tab.editor.set_selection(inner.start, inner.end);
                }
            }
            Command::Mark(style) => {
                let text = selection_or_word(tab);
                if !text.is_empty() {
                    tab.marks.set(style, Some(text));
                }
            }
            Command::ClearMark(style) => tab.marks.set(style, None),
            Command::ClearAllMarks => tab.marks.clear_all(),
            Command::FindCharsInRange => {
                self.dialog = Some(Dialog::CharRange {
                    preset: CharPreset::NonAscii,
                    from: "128".into(),
                    to: "255".into(),
                    status: None,
                });
            }
            Command::ToggleBookmark => {
                tab.editor.track_bookmarks(&tab.doc);
                let (line, _) = tab.editor.line_col(&tab.doc);
                tab.editor.toggle_bookmark(line);
            }
            Command::NextBookmark | Command::PreviousBookmark => {
                tab.editor.track_bookmarks(&tab.doc);
                tab.editor
                    .go_to_bookmark(&tab.doc, command == Command::NextBookmark);
            }
            Command::ClearBookmarks => tab.editor.clear_bookmarks(),
            Command::InvertBookmarks => {
                tab.editor.track_bookmarks(&tab.doc);
                let marked = tab.editor.bookmarks().clone();
                let lines = (0..tab.doc.text().len_lines()).filter(|line| !marked.contains(line));
                tab.editor.set_bookmarks(&tab.doc, lines);
            }
            Command::CopyBookmarked | Command::CutBookmarked => {
                tab.editor.track_bookmarks(&tab.doc);
                let rope = tab.doc.text();
                let lines: Vec<String> = tab
                    .editor
                    .bookmarks()
                    .iter()
                    .map(|&line| {
                        rope.line(line)
                            .to_string()
                            .trim_end_matches('\n')
                            .to_owned()
                    })
                    .collect();
                if !lines.is_empty() {
                    ctx.copy_text(lines.join("\n"));
                }
                if command == Command::CutBookmarked {
                    self.remove_bookmarked(false);
                }
            }
            Command::PasteToBookmarked => {
                self.paste_to_bookmarks = Some(ctx.input(|i| i.time));
                ctx.send_viewport_cmd(ViewportCommand::RequestPaste);
            }
            Command::RemoveBookmarked => self.remove_bookmarked(false),
            Command::RemoveUnbookmarked => self.remove_bookmarked(true),
            Command::BookmarkMatches => {
                tab.editor.track_bookmarks(&tab.doc);
                let rope = tab.doc.text();
                let mut lines = tab.editor.bookmarks().clone();
                lines.extend(
                    self.find
                        .matches(id, &tab.doc)
                        .iter()
                        .map(|found| rope.char_to_line(found.start)),
                );
                tab.editor.set_bookmarks(&tab.doc, lines);
            }
            Command::Summary => {
                let info = summary(&tab.doc);
                let mut text = String::new();
                if let Some(path) = tab.doc.path() {
                    text.push_str(&format!("{}: {}\n", texts.summary_path, path.display()));
                    let modified = std::fs::metadata(path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|time| jiff::Timestamp::try_from(time).ok())
                        .map(|time| {
                            time.to_zoned(jiff::tz::TimeZone::system())
                                .strftime("%Y-%m-%d %H:%M:%S")
                                .to_string()
                        });
                    if let Some(modified) = modified {
                        text.push_str(&format!("{}: {modified}\n", texts.summary_modified));
                    }
                    text.push('\n');
                }
                let rows = [
                    (texts.summary_chars, info.chars),
                    (texts.summary_words, info.words),
                    (texts.summary_lines, info.lines),
                    (texts.summary_bytes, info.bytes),
                    (texts.summary_selection, tab.editor.selection().len()),
                ];
                for (label, value) in rows {
                    text.push_str(&format!("{label}: {value}\n"));
                }
                self.dialog = Some(Dialog::Message {
                    title: texts.summary.trim_end_matches('…').to_owned(),
                    text,
                });
            }
            Command::ToggleSplit => {
                tab.split = match tab.split.take() {
                    Some(_) => None,
                    None => Some(Split {
                        other: tab.editor.clone(),
                        main_on_left: true,
                    }),
                };
            }
            Command::ToggleMonitoring => {
                tab.monitoring ^= true;
                tab.read_only = tab.monitoring;
                if tab.monitoring {
                    tab.editor.set_cursor(tab.doc.text().len_chars());
                }
            }
            Command::SetTabColor(color) => tab.color = color,
            Command::Convert(encoding) => tab.doc.set_encoding(encoding),
            Command::SetLineEnding(line_ending) => tab.doc.set_line_ending(line_ending),
            Command::SetSyntax(syntax) => tab.highlighter.set_language(syntax),
            Command::PlayMacro => {
                let events = self.macro_events.clone();
                tab.editor.play(events, Repeat::Times(1));
            }
            Command::PlaySavedMacro(index) => {
                if let Some(saved) = self.saved_macros.get(index) {
                    tab.editor.play(saved.events.clone(), Repeat::Times(1));
                }
            }
            Command::ChecksumSelection(algorithm) => {
                let text = tab.editor.selected_text(&tab.doc);
                if !text.is_empty() {
                    ctx.copy_text(checksum::of_text(algorithm, &text));
                }
            }
            // Handled in `run_global`.
            _ => {}
        }
    }

    pub(super) fn find_action(&mut self, action: FindAction) {
        match action {
            FindAction::FindAll => self.find_all(false),
            FindAction::FindAllInTabs => self.find_all(true),
            FindAction::ReplaceAllInTabs => {
                let (mut count, mut documents) = (0, 0);
                for tab in self.tabs.iter_mut().filter(|tab| !tab.read_only) {
                    let replaced = self.find.replace_all_in(&mut tab.doc);
                    if replaced > 0 {
                        count += replaced;
                        documents += 1;
                        let len = tab.doc.text().len_chars();
                        tab.editor.set_cursor(tab.editor.cursor().min(len));
                    }
                }
                let texts = self.texts();
                self.find.set_status(fill(
                    texts.replaced_in_docs,
                    &[
                        ("count", &count.to_string()),
                        ("files", &documents.to_string()),
                    ],
                ));
            }
            action => {
                let texts = self.texts();
                let tab = &mut self.tabs[self.active];
                self.find
                    .run(action, tab.id, &mut tab.doc, &mut tab.editor, texts);
                if action == FindAction::Close {
                    tab.editor.request_focus();
                }
            }
        }
    }

    /// Lists all matches of the find bar's query in the results panel.
    fn find_all(&mut self, all_tabs: bool) {
        if !self.find.has_pattern() {
            let tab = &self.tabs[self.active];
            let selected = tab.editor.selected_text(&tab.doc);
            self.find.show(false, &selected);
            return;
        }
        let texts = self.texts();
        let title = format!("“{}”", self.find.query().pattern);
        let mut results = Results::new(title);
        let indices: Vec<usize> = if all_tabs {
            (0..self.tabs.len()).collect()
        } else {
            vec![self.active]
        };
        for index in indices {
            let tab = &self.tabs[index];
            let matches = self.find.matches(tab.id, &tab.doc);
            if !matches.is_empty() {
                let lines = hits_by_line(&tab.doc, matches);
                results.add(Source::Tab(tab.id), tab.title(texts), lines);
            }
        }
        results.finish_search(texts);
        self.results = Some(results);
        self.show_results = true;
    }

    /// Removes bookmarked lines, or the others with `keep_marked`.
    fn remove_bookmarked(&mut self, keep_marked: bool) {
        let tab = self.tab();
        tab.editor.track_bookmarks(&tab.doc);
        let marked = tab.editor.bookmarks().clone();
        // Without bookmarks this would empty the whole document.
        if keep_marked && marked.is_empty() {
            return;
        }
        ops::retain_lines(&mut tab.doc, |line| marked.contains(&line) == keep_marked);
        let remaining = tab.doc.text().len_lines();
        let lines = if keep_marked { 0..remaining } else { 0..0 };
        tab.editor.set_bookmarks(&tab.doc, lines);
        tab.editor.set_cursor(0);
    }

    /// Delivers a requested paste to the bookmarked lines instead of the editor.
    pub(super) fn paste_into_bookmarks(&mut self, ctx: &egui::Context, now: f64) {
        let Some(asked) = self.paste_to_bookmarks else {
            return;
        };
        let pasted = ctx.input_mut(|input| {
            let index = input
                .events
                .iter()
                .position(|event| matches!(event, Event::Paste(_)))?;
            match input.events.remove(index) {
                Event::Paste(text) => Some(text),
                _ => None,
            }
        });
        let Some(text) = pasted else {
            if now - asked > 2.0 {
                self.paste_to_bookmarks = None;
            }
            return;
        };
        self.paste_to_bookmarks = None;
        let tab = self.tab();
        tab.editor.track_bookmarks(&tab.doc);
        let lines: Vec<usize> = tab.editor.bookmarks().iter().rev().copied().collect();
        tab.doc.seal_undo_group();
        for line in lines {
            let rope = tab.doc.text();
            let start = rope.line_to_char(line);
            let end = start + rope.line(line).chars().take_while(|&c| c != '\n').count();
            tab.doc.replace(start..end, &text);
        }
        tab.doc.seal_undo_group();
    }

    fn checksum_files(&mut self, algorithm: Algorithm) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        let Some(paths) = dialog.pick_files() else {
            return;
        };
        let lines = paths
            .iter()
            .map(|path| {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let hash = checksum::of_file(algorithm, path).unwrap_or_else(|err| err.to_string());
                (name, hash)
            })
            .collect();
        self.dialog = Some(Dialog::Checksums {
            title: algorithm.label().to_owned(),
            lines,
        });
    }
}
