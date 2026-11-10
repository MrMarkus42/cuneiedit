//! Several windows in one process. The main window is eframe's root viewport; the others
//! are immediate viewports. Each frame shows every window: the state of a window is swapped
//! into `CuneiEditApp`, its frame runs, and the state is swapped back out.
//!
//! Closing a window asks about its unsaved documents and closes it. Closing the main window
//! while others are open moves the first other window into the main window's place. "Quit"
//! asks about unsaved documents in all windows (unless they are kept in backups) and ends the
//! program.

use eframe::egui::{self, Ui, ViewportBuilder, ViewportCommand, ViewportId, WindowLevel};

use super::dialogs::Dialog;
use super::tab::Tab;
use super::{CuneiEditApp, WindowState};

impl CuneiEditApp {
    pub(super) fn all_windows_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let focused = ctx.input(|i| {
            i.raw
                .viewports
                .iter()
                .find(|(_, info)| info.focused == Some(true))
                .map(|(id, _)| *id)
        });
        if let Some(id) = focused {
            self.last_focused = id;
        }

        self.window_ui(ui);
        for index in 0..self.windows.len() {
            self.show_other_window(&ctx, index);
        }

        self.windows.retain(|window| !window.closed_window);
        if self.closed_window {
            self.replace_main_window(&ctx);
        }
        let known = |id: ViewportId| {
            id == ViewportId::ROOT || self.windows.iter().any(|window| window.viewport == id)
        };
        if !known(self.last_focused) {
            self.last_focused = ViewportId::ROOT;
        }
        if self.app_quitting && !self.quitting && self.windows.iter().all(|w| !w.quitting) {
            self.allow_close = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn show_other_window(&mut self, ctx: &egui::Context, index: usize) {
        let mut window = std::mem::take(&mut self.windows[index]);
        self.swap_window(&mut window);
        self.windows[index] = window;

        let builder = ViewportBuilder::default()
            .with_title(self.window_title.clone())
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([400.0, 300.0])
            .with_drag_and_drop(true)
            .with_icon(self.icon.clone());
        ctx.show_viewport_immediate(self.viewport, builder, |ui, _| {
            self.window_ui(ui);
        });

        let mut window = std::mem::take(&mut self.windows[index]);
        self.swap_window(&mut window);
        self.windows[index] = window;
    }

    /// The main window was closed while others are open: the first other window moves into
    /// the main window (its contents, position and size), and its own window goes away.
    fn replace_main_window(&mut self, ctx: &egui::Context) {
        if self.windows.is_empty() {
            self.closed_window = false;
            return;
        }
        let mut window = self.windows.remove(0);
        let id = window.viewport;
        let (outer, inner) = ctx.input(|i| {
            i.raw
                .viewports
                .get(&id)
                .map_or((None, None), |info| (info.outer_rect, info.inner_rect))
        });
        self.swap_window(&mut window);
        self.viewport = ViewportId::ROOT;
        self.window_title.clear();
        if let Some(rect) = outer {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(rect.min));
        }
        if let Some(rect) = inner {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(rect.size()));
        }
        let level = if self.always_on_top || self.post_it {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        };
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
        if self.last_focused == id {
            self.last_focused = ViewportId::ROOT;
        }
        self.raise = true;
    }

    /// Opens a new window with `tabs` (or an empty document), showing the tab at `active`.
    pub(super) fn open_window(&mut self, tabs: Vec<Tab>, active: usize) {
        let number = self.next_window;
        self.next_window += 1;
        let mut window = WindowState {
            viewport: ViewportId::from_hash_of(("cuneiedit-window", number)),
            number,
            ..WindowState::default()
        };
        // The new window's tabs are set up with the usual tab code.
        self.swap_window(&mut window);
        self.tabs = tabs;
        if self.tabs.is_empty() {
            self.new_tab();
        }
        self.activate(active.min(self.tabs.len() - 1));
        self.swap_window(&mut window);
        self.windows.push(window);
    }

    /// Moves the active tab into a new window.
    pub(super) fn move_to_new_window(&mut self) {
        if self.tabs.len() == 1 && self.tabs[0].is_pristine() {
            return;
        }
        let tab = self.detach_tab(self.active);
        self.open_window(vec![tab], 0);
    }

    /// A window's close button, or "Quit" for the main window.
    pub(super) fn close_requested(&mut self, ctx: &egui::Context) {
        if self.viewport != ViewportId::ROOT {
            if !self.closing && !self.quitting && self.dialog.is_none() {
                self.close_window(ctx);
            }
            return;
        }
        if self.allow_close {
            return;
        }
        if self.windows.is_empty() || self.quit_requested {
            self.request_quit(ctx);
            return;
        }
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        if !self.closing && !self.quitting && self.dialog.is_none() {
            self.close_window(ctx);
        }
    }

    fn close_window(&mut self, ctx: &egui::Context) {
        self.closing = true;
        self.close_queue = self.tabs.iter().map(|tab| tab.id).collect();
        self.process_close_queue(ctx);
    }

    /// "Quit": closes the main window, which asks about unsaved documents everywhere.
    pub(super) fn quit(&mut self, ctx: &egui::Context) {
        self.quit_requested = true;
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
    }

    /// Starts asking about unsaved documents in every window before quitting. Returns
    /// `false` if a window shows a dialog, so quitting has to wait.
    pub(super) fn ask_all_windows(&mut self) -> bool {
        let busy = self.dialog.is_some() || self.windows.iter().any(|w| w.dialog.is_some());
        if busy {
            return false;
        }
        self.app_quitting = true;
        let unsaved = |tabs: &[Tab]| -> std::collections::VecDeque<u64> {
            tabs.iter()
                .filter(|tab| tab.doc.is_modified())
                .map(|tab| tab.id)
                .collect()
        };
        self.close_queue = unsaved(&self.tabs);
        self.quitting = !self.close_queue.is_empty();
        for window in &mut self.windows {
            window.close_queue = unsaved(&window.tabs);
            window.quitting = !window.close_queue.is_empty();
        }
        true
    }

    /// The user cancelled closing or quitting: every window stops asking.
    pub(super) fn cancel_closing(&mut self) {
        self.close_queue.clear();
        self.quitting = false;
        self.closing = false;
        self.app_quitting = false;
        self.quit_requested = false;
        for window in &mut self.windows {
            if window.quitting {
                window.quitting = false;
                window.close_queue.clear();
                if matches!(window.dialog, Some(Dialog::Unsaved { .. })) {
                    window.dialog = None;
                }
            }
        }
        super::update::cancel_relaunch();
    }

    /// Finds a tab with `path` in another window and shows it there.
    pub(super) fn show_in_other_window(&mut self, path: &std::path::Path) -> bool {
        for window in &mut self.windows {
            if let Some(index) = window
                .tabs
                .iter()
                .position(|tab| tab.doc.path() == Some(path))
            {
                window.active = index;
                window.tabs[index].editor.request_focus();
                window.raise = true;
                return true;
            }
        }
        false
    }

    /// The tabs of all windows, in session order, with the active index of each window.
    pub(super) fn all_tabs_mut(&mut self) -> Vec<(usize, &mut Vec<Tab>)> {
        let mut groups = vec![(self.number, self.active, &mut self.tabs)];
        groups.extend(
            self.windows
                .iter_mut()
                .map(|window| (window.number, window.active, &mut window.tabs)),
        );
        groups.sort_by_key(|(number, ..)| *number);
        groups
            .into_iter()
            .map(|(_, active, tabs)| (active, tabs))
            .collect()
    }
}
