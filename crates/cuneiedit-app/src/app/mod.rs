//! The main window: tabs, menus, panels and dialogs.
//!
//! - `command`: every command with label and shortcut; `keymap`: shortcuts in effect
//! - `run`: what each command does
//! - `files`: opening, saving, closing, sessions, backups, external changes, export
//! - `menu`: menu bar, tab bar, status bar; `toolbar`: icon bar with own drawn icons;
//!   `mac_menu`: native macOS menu bar and files opened from Finder
//! - `editor_view`: the editor area, split view, second view, completion and call tips
//! - `panels`: search results, function list, document list, clipboard, characters
//! - `navigation`: file tree and document map
//! - `dialogs`, `preferences`: modal dialogs
//! - `compare_view`: side-by-side comparison

mod command;
mod compare_view;
mod dialogs;
mod editor_view;
mod file_search;
mod files;
mod keymap;
#[cfg(target_os = "macos")]
mod mac_menu;
mod menu;
mod navigation;
mod panels;
mod preferences;
mod run;
mod tab;
mod toolbar;
pub mod update;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use cuneiedit_core::Document;
use cuneiedit_core::highlight::{self, UserLanguage};
use cuneiedit_editor::Palette;
use eframe::egui::{self, CentralPanel, Frame, Key, Modifiers, Panel, Ui, ViewportCommand};

use self::command::{Command, TAB_KEYS};
use self::dialogs::Dialog;
use self::editor_view::{CallTip, Completion};
use self::file_search::{FileSearch, FilesForm};
use self::keymap::Keymap;
use self::navigation::FileTree;
use self::panels::{Results, SymbolCache};
use self::tab::Tab;
use crate::compare::CompareView;
use crate::find::{FindAction, FindBar};
use crate::i18n::Texts;
use crate::macros::SavedMacro;
use crate::settings::{Flag, Settings};
use crate::{languages, macros, themes};

const DISK_CHECK_SECONDS: f64 = 2.0;
const BACKUP_SECONDS: f64 = 10.0;
const MAX_CLOSED: usize = 20;
const MAX_CLIPBOARD: usize = 30;

pub struct CuneiEditApp {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    settings: Settings,
    dialog: Option<Dialog>,
    find: FindBar,
    compare: Option<CompareView>,
    compare_first: Option<u64>,
    /// Tabs waiting to be closed; unsaved ones are asked about one by one.
    close_queue: VecDeque<u64>,
    quitting: bool,
    allow_close: bool,
    window_title: String,
    /// Recently closed files, most recent last.
    closed: Vec<PathBuf>,
    results: Option<Results>,
    show_results: bool,
    file_search: Option<FileSearch>,
    files_form: FilesForm,
    completion: Option<Completion>,
    editor_focused: bool,
    macro_events: Vec<egui::Event>,
    /// Tab whose editor is recording a macro.
    recording: Option<u64>,
    /// Time "replace bookmarked lines" asked for the clipboard contents.
    paste_to_bookmarks: Option<f64>,
    /// Copied texts, most recent first. Kept in memory only.
    clipboard: VecDeque<String>,
    symbols: SymbolCache,
    always_on_top: bool,
    fullscreen: bool,
    last_disk_check: f64,
    last_backup: f64,
    keymap: Keymap,
    saved_macros: Vec<SavedMacro>,
    user_languages: Vec<UserLanguage>,
    tree: FileTree,
    call_tip: Option<CallTip>,
    /// Only the text is shown, centered, in full screen.
    distraction_free: bool,
    /// Only the text is shown, in a window that stays on top.
    post_it: bool,
    /// Tab shown in the right view after "move to other view".
    second_view: Option<u64>,
    /// Tab last shown in the left view, while the right one is active.
    left_view: Option<u64>,
    #[cfg(target_os = "macos")]
    mac_menu: Option<mac_menu::MacMenu>,
    updater: update::Updater,
}

impl CuneiEditApp {
    pub fn new(ctx: &egui::Context, files: Vec<PathBuf>) -> Self {
        let settings = Settings::load();
        ctx.set_theme(settings.theme);
        // The editor zooms its own font instead of the whole interface.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        let user_languages = languages::load();
        highlight::set_user_languages(&user_languages);
        let font_error = files::apply_font(ctx, settings.font_path.as_deref()).err();

        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            keymap: Keymap::new(&settings.shortcuts),
            settings,
            dialog: None,
            find: FindBar::default(),
            compare: None,
            compare_first: None,
            close_queue: VecDeque::new(),
            quitting: false,
            allow_close: false,
            window_title: String::new(),
            closed: Vec::new(),
            results: None,
            show_results: false,
            file_search: None,
            files_form: FilesForm::default(),
            completion: None,
            editor_focused: false,
            macro_events: Vec::new(),
            recording: None,
            paste_to_bookmarks: None,
            clipboard: VecDeque::new(),
            symbols: SymbolCache::default(),
            always_on_top: false,
            fullscreen: false,
            last_disk_check: 0.0,
            last_backup: 0.0,
            saved_macros: macros::load(),
            user_languages,
            tree: FileTree::default(),
            call_tip: None,
            distraction_free: false,
            post_it: false,
            second_view: None,
            left_view: None,
            #[cfg(target_os = "macos")]
            mac_menu: None,
            updater: update::Updater::default(),
        };
        app.restore_session();
        if app.tabs.is_empty() {
            app.new_tab();
        }
        for file in files {
            app.open_path(&file);
        }
        if let Some(error) = font_error {
            let message = error.message(app.texts());
            app.error(message);
        }
        app
    }

    fn texts(&self) -> &'static Texts {
        self.settings.language.texts()
    }

    /// Colors of the text area: the chosen theme, or `None` to follow the appearance.
    fn palette(&self) -> Option<Palette> {
        themes::palette(&self.settings.color_theme)
    }

    fn persist(&self) {
        // Losing preferences is not worth interrupting the user for.
        let _ = self.settings.save();
    }

    fn tab(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn push_tab(&mut self, doc: Document, untitled_number: usize) -> usize {
        self.next_id += 1;
        self.tabs.push(Tab::new(self.next_id, doc, untitled_number));
        let index = self.tabs.len() - 1;
        self.activate(index);
        index
    }

    fn new_tab(&mut self) {
        let number = lowest_free_number(
            self.tabs
                .iter()
                .filter(|tab| tab.doc.path().is_none())
                .map(|tab| tab.untitled_number),
        );
        let doc = Document::with_format(self.settings.new_encoding, self.settings.new_line_ending);
        self.push_tab(doc, number);
    }

    fn activate(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
            self.tab().editor.request_focus();
        }
    }

    fn error(&mut self, message: String) {
        self.dialog = Some(Dialog::Message {
            title: self.texts().error.to_owned(),
            text: message,
        });
    }

    fn remove_tab(&mut self, index: usize) {
        let tab = self.tabs.remove(index);
        if self
            .compare
            .as_ref()
            .is_some_and(|view| view.uses_tab(tab.id))
        {
            self.compare = None;
        }
        if self.compare_first == Some(tab.id) {
            self.compare_first = None;
        }
        if self.recording == Some(tab.id) {
            self.recording = None;
        }
        if self.completion.as_ref().is_some_and(|c| c.tab == tab.id) {
            self.completion = None;
        }
        if self.call_tip.as_ref().is_some_and(|c| c.tab == tab.id) {
            self.call_tip = None;
        }
        if self.second_view == Some(tab.id) {
            self.second_view = None;
        }
        if self.left_view == Some(tab.id) {
            self.left_view = None;
        }
        if let Some(path) = tab.doc.path() {
            self.closed.retain(|closed| closed != path);
            self.closed.push(path.to_path_buf());
            if self.closed.len() > MAX_CLOSED {
                self.closed.remove(0);
            }
        }
        if self.tabs.is_empty() {
            self.new_tab();
            return;
        }
        let active = if self.active > index || self.active >= self.tabs.len() {
            self.active.saturating_sub(1)
        } else {
            self.active
        };
        self.activate(active);
    }

    fn shortcut_command(&self, ctx: &egui::Context) -> Option<Command> {
        if self.dialog.is_some() {
            return None;
        }
        let leave_mode = (self.distraction_free || self.post_it)
            && self.completion.is_none()
            && self.call_tip.is_none()
            && !self.find.open
            && !self.tabs[self.active].editor.has_multiple_selections();
        ctx.input_mut(|input| {
            if leave_mode && input.consume_key(Modifiers::NONE, Key::Escape) {
                return Some(if self.post_it {
                    Command::PostIt
                } else {
                    Command::DistractionFree
                });
            }
            let tab = TAB_KEYS
                .iter()
                .position(|&key| input.consume_key(Modifiers::COMMAND, key));
            if let Some(index) = tab {
                // Cmd+9 is the last tab, as in browsers.
                let index = if index == 8 {
                    self.tabs.len() - 1
                } else {
                    index
                };
                return Some(Command::Activate(index));
            }
            self.keymap
                .entries()
                .iter()
                .find(|(shortcut, _)| input.consume_shortcut(shortcut))
                .map(|(_, command)| match command {
                    Command::StartRecording if self.recording.is_some() => Command::StopRecording,
                    command => *command,
                })
        })
    }

    fn handle_window(&mut self, ctx: &egui::Context) {
        self.fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            self.request_quit(ctx);
        }

        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        for path in dropped {
            self.open_path(&path);
        }

        let tab = &self.tabs[self.active];
        let marker = if tab.doc.is_modified() { "● " } else { "" };
        let title = format!("{marker}{} – CuneiEdit", tab.title(self.texts()));
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    /// Disk checks, backups and the file search, at their own pace.
    fn background_work(&mut self, ctx: &egui::Context, now: f64) {
        if now - self.last_disk_check >= DISK_CHECK_SECONDS {
            self.last_disk_check = now;
            self.check_disk();
        }
        if now - self.last_backup >= BACKUP_SECONDS {
            self.last_backup = now;
            self.backup_if_needed();
        }
        if let Some(search) = &mut self.file_search {
            let found = search.poll();
            if let Some(results) = &mut self.results {
                for hits in found {
                    results.add_file(hits);
                }
            }
            if search.done {
                if let Some(results) = &mut self.results {
                    results.finish(search, self.settings.language.texts());
                }
                self.file_search = None;
            }
        }
        // Keep checking even without input: other programs may change files.
        let wait = if self.file_search.is_some() {
            0.1
        } else {
            DISK_CHECK_SECONDS
        };
        ctx.request_repaint_after(Duration::from_secs_f64(wait));
    }

    /// Remembers what was copied, while the clipboard history is shown.
    fn record_clipboard(&mut self, ctx: &egui::Context) {
        if !self.settings.get(Flag::ClipboardHistory) {
            return;
        }
        let copied: Vec<String> = ctx.output(|o| {
            o.commands
                .iter()
                .filter_map(|command| match command {
                    egui::OutputCommand::CopyText(text) if !text.is_empty() => Some(text.clone()),
                    _ => None,
                })
                .collect()
        });
        for text in copied {
            self.clipboard.retain(|entry| *entry != text);
            self.clipboard.push_front(text);
            self.clipboard.truncate(MAX_CLIPBOARD);
        }
    }
}

/// The smallest number from 1 up that is not in `used`, e.g. for "new 1", "new 2".
fn lowest_free_number(used: impl Iterator<Item = usize>) -> usize {
    let used: std::collections::BTreeSet<usize> = used.collect();
    (1..).find(|n| !used.contains(n)).unwrap_or(1)
}

impl eframe::App for CuneiEditApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.handle_window(&ctx);
        self.background_work(&ctx, now);
        self.update_tick(&ctx);
        let texts = self.settings.language.texts();

        let mut commands = Vec::new();
        commands.extend(self.shortcut_command(&ctx));
        #[cfg(target_os = "macos")]
        self.mac_menu_frame(&ctx, &mut commands);
        self.completion_keys(&ctx);
        self.paste_into_bookmarks(&ctx, now);
        let chrome = !self.distraction_free && !self.post_it;
        if chrome {
            Panel::top("cuneiedit-menu").show(ui, |ui| commands.extend(self.menu_bar(ui)));
            if self.settings.get(Flag::Toolbar) {
                Panel::top("cuneiedit-toolbar").show(ui, |ui| commands.extend(self.toolbar(ui)));
            }
            Panel::top("cuneiedit-tabs").show(ui, |ui| commands.extend(self.tab_bar(ui)));
            Panel::bottom("cuneiedit-status").show(ui, |ui| self.status_bar(ui));
        }

        let mut find_action = None;
        if self.find.open {
            if self.dialog.is_none()
                && self.completion.is_none()
                && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
            {
                find_action = Some(FindAction::Close);
            }
            Panel::bottom("cuneiedit-find").show(ui, |ui| {
                let selection = self.tabs[self.active].editor.selection();
                find_action = find_action.or(self.find.ui(ui, texts, &selection));
            });
        }
        if let Some(action) = find_action {
            self.find_action(action);
        }
        for command in std::mem::take(&mut commands) {
            self.run(&ctx, command);
        }

        self.side_panels(ui, &mut commands, chrome);
        if self.show_results && self.results.is_some() && chrome {
            Panel::bottom("cuneiedit-results")
                .resizable(true)
                .default_size(180.0)
                .show(ui, |ui| self.results_panel(ui, &mut commands));
        }
        if self.compare.is_some() {
            Panel::bottom("cuneiedit-compare").show(ui, |ui| commands.extend(self.compare_bar(ui)));
        }
        let fill = self
            .palette()
            .map_or(ui.visuals().extreme_bg_color, |p| p.background);
        CentralPanel::default()
            .frame(Frame::NONE.fill(fill))
            .show(ui, |ui| {
                if self.compare.is_some() {
                    self.show_compare(ui);
                } else {
                    self.show_editor(ui, &mut commands);
                }
            });
        for command in commands {
            self.run(&ctx, command);
        }

        self.completion_popup(&ctx);
        self.call_tip_popup(&ctx);
        self.show_dialog(&ctx);
        self.record_clipboard(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_the_lowest_free_untitled_number() {
        assert_eq!(lowest_free_number([].into_iter()), 1);
        assert_eq!(lowest_free_number([1, 2, 4].into_iter()), 3);
        assert_eq!(lowest_free_number([2].into_iter()), 1);
    }
}
