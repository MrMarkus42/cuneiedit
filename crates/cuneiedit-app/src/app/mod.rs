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
//! - `windows`: several windows in one process

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
mod windows;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use cuneiedit_core::Document;
use cuneiedit_core::highlight::{self, UserLanguage};
use cuneiedit_editor::Palette;
use eframe::egui::{
    self, CentralPanel, Frame, IconData, Key, Modifiers, Panel, Ui, ViewportCommand, ViewportId,
};

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
/// Size of the window icon in pixels.
pub const ICON_SIZE: u32 = 256;

/// Declares `CuneiEditApp` from the state all windows share and the state of one window.
/// The window being shown keeps its state in `CuneiEditApp` itself; the other windows keep
/// theirs in `WindowState`s and are swapped in to be shown (see `windows.rs`).
macro_rules! app_state {
    (
        shared { $($(#[$s_meta:meta])* $s:ident: $s_ty:ty,)* }
        window { $($(#[$w_meta:meta])* $w:ident: $w_ty:ty = $w_init:expr,)* }
    ) => {
        pub struct CuneiEditApp {
            $($(#[$s_meta])* $s: $s_ty,)*
            $($(#[$w_meta])* $w: $w_ty,)*
        }

        struct Shared {
            $($s: $s_ty,)*
        }

        struct WindowState {
            $($w: $w_ty,)*
        }

        impl Default for WindowState {
            fn default() -> Self {
                Self { $($w: $w_init,)* }
            }
        }

        impl CuneiEditApp {
            fn assemble(shared: Shared, window: WindowState) -> Self {
                Self { $($s: shared.$s,)* $($w: window.$w,)* }
            }

            fn swap_window(&mut self, window: &mut WindowState) {
                $(std::mem::swap(&mut self.$w, &mut window.$w);)*
            }
        }
    };
}

app_state! {
    shared {
        next_id: u64,
        settings: Settings,
        /// Set by "Quit": closing the main window then quits instead of closing the window.
        quit_requested: bool,
        /// Asking about unsaved documents in all windows before quitting.
        app_quitting: bool,
        allow_close: bool,
        /// Recently closed files, most recent last.
        closed: Vec<PathBuf>,
        macro_events: Vec<egui::Event>,
        /// Tab whose editor is recording a macro.
        recording: Option<u64>,
        /// Copied texts, most recent first. Kept in memory only.
        clipboard: VecDeque<String>,
        last_backup: f64,
        keymap: Keymap,
        saved_macros: Vec<SavedMacro>,
        user_languages: Vec<UserLanguage>,
        #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
        mac_menu: MacMenuState,
        updater: update::Updater,
        /// Files handed over by later starts.
        incoming: Option<Receiver<Vec<PathBuf>>>,
        /// The windows not being shown right now.
        windows: Vec<WindowState>,
        /// Window that had the focus last; files from outside open there.
        last_focused: ViewportId,
        next_window: u64,
        icon: Arc<IconData>,
    }
    window {
        viewport: ViewportId = ViewportId::ROOT,
        /// Order of the windows in the session; the main window comes first.
        number: u64 = 0,
        tabs: Vec<Tab> = Vec::new(),
        active: usize = 0,
        dialog: Option<Dialog> = None,
        find: FindBar = FindBar::default(),
        compare: Option<CompareView> = None,
        compare_first: Option<u64> = None,
        /// Tabs waiting to be closed; unsaved ones are asked about one by one.
        close_queue: VecDeque<u64> = VecDeque::new(),
        /// Asking about this window's unsaved documents before quitting.
        quitting: bool = false,
        /// Closing this window: its tabs are closed one by one.
        closing: bool = false,
        /// All tabs are closed; the window goes away.
        closed_window: bool = false,
        /// Bring this window to the front.
        raise: bool = false,
        window_title: String = String::new(),
        results: Option<Results> = None,
        show_results: bool = false,
        file_search: Option<FileSearch> = None,
        files_form: FilesForm = FilesForm::default(),
        completion: Option<Completion> = None,
        editor_focused: bool = false,
        /// Time "replace bookmarked lines" asked for the clipboard contents.
        paste_to_bookmarks: Option<f64> = None,
        symbols: SymbolCache = SymbolCache::default(),
        always_on_top: bool = false,
        fullscreen: bool = false,
        last_disk_check: f64 = 0.0,
        tree: FileTree = FileTree::default(),
        call_tip: Option<CallTip> = None,
        /// Only the text is shown, centered, in full screen.
        distraction_free: bool = false,
        /// Only the text is shown, in a window that stays on top.
        post_it: bool = false,
        /// Tab shown in the right view after "move to other view".
        second_view: Option<u64> = None,
        /// Tab last shown in the left view, while the right one is active.
        left_view: Option<u64> = None,
    }
}

#[cfg(target_os = "macos")]
type MacMenuState = Option<mac_menu::MacMenu>;
#[cfg(not(target_os = "macos"))]
type MacMenuState = ();

impl CuneiEditApp {
    pub fn new(
        ctx: &egui::Context,
        files: Vec<PathBuf>,
        incoming: Option<Receiver<Vec<PathBuf>>>,
    ) -> Self {
        let settings = Settings::load();
        ctx.set_theme(settings.theme);
        // The editor zooms its own font instead of the whole interface.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        let user_languages = languages::load();
        highlight::set_user_languages(&user_languages);
        let font_error = files::apply_font(ctx, settings.font_path.as_deref()).err();

        let shared = Shared {
            next_id: 0,
            keymap: Keymap::new(&settings.shortcuts),
            settings,
            quit_requested: false,
            app_quitting: false,
            allow_close: false,
            closed: Vec::new(),
            macro_events: Vec::new(),
            recording: None,
            clipboard: VecDeque::new(),
            last_backup: 0.0,
            saved_macros: macros::load(),
            user_languages,
            mac_menu: MacMenuState::default(),
            updater: update::Updater::default(),
            incoming,
            windows: Vec::new(),
            last_focused: ViewportId::ROOT,
            next_window: 1,
            icon: Arc::new(IconData {
                rgba: crate::app_icon::rgba(ICON_SIZE),
                width: ICON_SIZE,
                height: ICON_SIZE,
            }),
        };
        let mut app = Self::assemble(shared, WindowState::default());
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
        let tab = self.detach_tab(index);
        if self.recording == Some(tab.id) {
            self.recording = None;
        }
        if let Some(path) = tab.doc.path() {
            self.closed.retain(|closed| closed != path);
            self.closed.push(path.to_path_buf());
            if self.closed.len() > MAX_CLOSED {
                self.closed.remove(0);
            }
        }
    }

    /// Takes a tab out of this window, keeping at least one tab open.
    fn detach_tab(&mut self, index: usize) -> Tab {
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
        if self.tabs.is_empty() {
            self.new_tab();
            return tab;
        }
        let active = if self.active > index || self.active >= self.tabs.len() {
            self.active.saturating_sub(1)
        } else {
            self.active
        };
        self.activate(active);
        tab
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
        if ctx.input(|i| i.viewport().close_requested()) {
            self.close_requested(ctx);
        }
        if self.quitting && self.dialog.is_none() {
            self.process_close_queue(ctx);
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

        if self.viewport == self.last_focused {
            let handed_over: Vec<Vec<PathBuf>> = self
                .incoming
                .as_ref()
                .map(|incoming| incoming.try_iter().collect())
                .unwrap_or_default();
            if !handed_over.is_empty() {
                for path in handed_over.into_iter().flatten() {
                    self.open_path(&path);
                }
                self.raise = true;
            }
        }
        if std::mem::take(&mut self.raise) {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
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
        if self.viewport == ViewportId::ROOT && now - self.last_backup >= BACKUP_SECONDS {
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
        self.all_windows_ui(ui);
    }
}

impl CuneiEditApp {
    /// One frame of the window whose state is in `self`.
    fn window_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.handle_window(&ctx);
        self.background_work(&ctx, now);
        if self.viewport == ViewportId::ROOT {
            self.update_tick(&ctx);
        }
        let texts = self.settings.language.texts();

        let mut commands = Vec::new();
        commands.extend(self.shortcut_command(&ctx));
        #[cfg(target_os = "macos")]
        if self.viewport == self.last_focused {
            self.mac_menu_frame(&ctx, &mut commands);
        }
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
