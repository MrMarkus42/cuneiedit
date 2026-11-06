//! The native macOS menu bar with the usual menus (app, File, Edit, View, Window, Help) and
//! files opened from Finder. The complete menus stay in the window. Clipboard and undo items
//! are passed on as input events, so they also work in text fields.

use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui::{self, Event, Key, KeyboardShortcut, Modifiers, ViewportCommand};
use muda::accelerator::{Accelerator, Code, Modifiers as MenuModifiers};
use muda::{IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use super::PilcrowApp;
use super::command::Command;
use super::keymap::Keymap;
use crate::i18n::{Language, Texts};
use crate::settings::Flag;

/// What a click in the menu bar asks for.
enum Action {
    Run(Command),
    Input(Event),
    Paste,
}

pub struct MacMenu {
    /// Kept alive while it is the application's menu.
    _menu: Menu,
    language: Language,
    shortcuts: Vec<(String, String)>,
    items: Vec<(MenuItem, Command)>,
    enabled: Vec<bool>,
    clicked: Arc<Mutex<Vec<MenuId>>>,
}

fn code(key: Key) -> Option<Code> {
    let name = key.name();
    let single = name.chars().count() == 1;
    if single && name.chars().all(|c| c.is_ascii_alphabetic()) {
        return format!("Key{name}").parse().ok();
    }
    if single && name.chars().all(|c| c.is_ascii_digit()) {
        return format!("Digit{name}").parse().ok();
    }
    if name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit()) {
        return name.parse().ok();
    }
    Some(match key {
        Key::ArrowUp => Code::ArrowUp,
        Key::ArrowDown => Code::ArrowDown,
        Key::ArrowLeft => Code::ArrowLeft,
        Key::ArrowRight => Code::ArrowRight,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::Home => Code::Home,
        Key::End => Code::End,
        Key::Tab => Code::Tab,
        Key::Space => Code::Space,
        Key::Enter => Code::Enter,
        Key::Backspace => Code::Backspace,
        Key::Delete => Code::Delete,
        Key::Comma => Code::Comma,
        Key::Period => Code::Period,
        Key::Minus => Code::Minus,
        Key::Equals | Key::Plus => Code::Equal,
        Key::Slash => Code::Slash,
        Key::OpenBracket => Code::BracketLeft,
        Key::CloseBracket => Code::BracketRight,
        _ => return None,
    })
}

fn accelerator(shortcut: KeyboardShortcut) -> Option<Accelerator> {
    let m = shortcut.modifiers;
    let mut modifiers = MenuModifiers::empty();
    if m.command || m.mac_cmd {
        modifiers |= MenuModifiers::META;
    }
    if m.ctrl && !m.command {
        modifiers |= MenuModifiers::CONTROL;
    }
    if m.alt {
        modifiers |= MenuModifiers::ALT;
    }
    if m.shift {
        modifiers |= MenuModifiers::SHIFT;
    }
    Some(Accelerator::new(modifiers, code(shortcut.logical_key)?))
}

fn key_event(key: Key, shift: bool) -> Event {
    let mut modifiers = Modifiers::MAC_CMD | Modifiers::COMMAND;
    modifiers.shift = shift;
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// Edit commands that text fields handle themselves, as the input they expect.
fn as_input(command: Command) -> Option<Action> {
    Some(match command {
        Command::Undo => Action::Input(key_event(Key::Z, false)),
        Command::Redo => Action::Input(key_event(Key::Z, true)),
        Command::SelectAll => Action::Input(key_event(Key::A, false)),
        Command::Cut => Action::Input(Event::Cut),
        Command::Copy => Action::Input(Event::Copy),
        Command::Paste => Action::Paste,
        _ => return None,
    })
}

struct Builder<'a> {
    texts: &'static Texts,
    keymap: &'a Keymap,
    items: Vec<(MenuItem, Command)>,
}

impl Builder<'_> {
    fn item(&mut self, command: Command, label: Option<&str>) -> MenuItem {
        let text = label.map_or_else(|| command.label(self.texts).into_owned(), str::to_owned);
        let id = MenuId::new(format!("pilcrow-{}", self.items.len()));
        let accelerator = self.keymap.get(command).and_then(accelerator);
        let item = MenuItem::with_id(id, text, true, accelerator);
        self.items.push((item.clone(), command));
        item
    }

    fn submenu(&mut self, title: &str, entries: &[Option<Command>]) -> Submenu {
        let submenu = Submenu::new(title, true);
        for entry in entries {
            match entry {
                Some(command) => {
                    let item = self.item(*command, None);
                    let _ = submenu.append(&item);
                }
                None => {
                    let _ = submenu.append(&PredefinedMenuItem::separator());
                }
            }
        }
        submenu
    }
}

impl MacMenu {
    pub fn new(ctx: &egui::Context, language: Language, keymap: &Keymap) -> Self {
        let t = language.texts();
        let mut builder = Builder {
            texts: t,
            keymap,
            items: Vec::new(),
        };
        let separator = PredefinedMenuItem::separator;

        let app = Submenu::new("Pilcrow", true);
        let about = builder.item(Command::About, None);
        let settings = builder.item(Command::Preferences, Some(t.mac_settings));
        let quit = builder.item(Command::Quit, Some(t.mac_quit));
        let app_items: [&dyn IsMenuItem; 11] = [
            &about,
            &separator(),
            &settings,
            &separator(),
            &PredefinedMenuItem::services(Some(t.mac_services)),
            &separator(),
            &PredefinedMenuItem::hide(Some(t.mac_hide)),
            &PredefinedMenuItem::hide_others(Some(t.mac_hide_others)),
            &PredefinedMenuItem::show_all(Some(t.mac_show_all)),
            &separator(),
            &quit,
        ];
        let _ = app.append_items(&app_items);

        let file = builder.submenu(
            t.menu_file,
            &[
                Some(Command::New),
                Some(Command::Open),
                None,
                Some(Command::Close),
                Some(Command::Save),
                Some(Command::SaveAs),
                Some(Command::SaveAll),
                None,
                Some(Command::Print),
            ],
        );
        let edit = builder.submenu(
            t.menu_edit,
            &[
                Some(Command::Undo),
                Some(Command::Redo),
                None,
                Some(Command::Cut),
                Some(Command::Copy),
                Some(Command::Paste),
                Some(Command::SelectAll),
                None,
                Some(Command::OpenFind),
                Some(Command::OpenReplace),
                Some(Command::FindNext),
                Some(Command::FindPrevious),
            ],
        );
        let view = builder.submenu(
            t.menu_view,
            &[
                Some(Command::Toggle(Flag::Toolbar)),
                Some(Command::Toggle(Flag::WordWrap)),
                None,
                Some(Command::ZoomIn),
                Some(Command::ZoomOut),
                Some(Command::ZoomReset),
            ],
        );
        let _ = view.append_items(&[
            &separator(),
            &PredefinedMenuItem::fullscreen(Some(t.mac_fullscreen)),
        ]);
        let window = Submenu::new(t.menu_window, true);
        let _ = window.append_items(&[
            &PredefinedMenuItem::minimize(Some(t.mac_minimize)),
            &PredefinedMenuItem::maximize(Some(t.mac_zoom)),
            &separator(),
            &PredefinedMenuItem::bring_all_to_front(Some(t.mac_bring_all_to_front)),
        ]);
        let help = builder.submenu(t.menu_help, &[Some(Command::About)]);

        let menu = Menu::new();
        let _ = menu.append_items(&[&app, &file, &edit, &view, &window, &help]);
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();

        let clicked = Arc::new(Mutex::new(Vec::new()));
        let queue = Arc::clone(&clicked);
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(event.id);
            ctx.request_repaint();
        }));

        Self {
            _menu: menu,
            language,
            shortcuts: Vec::new(),
            enabled: vec![true; builder.items.len()],
            items: builder.items,
            clicked,
        }
    }

    fn poll(&self) -> Vec<Action> {
        let ids = std::mem::take(&mut *self.clicked.lock().unwrap_or_else(PoisonError::into_inner));
        ids.iter()
            .filter_map(|id| self.items.iter().find(|(item, _)| item.id() == id))
            .map(|&(_, command)| as_input(command).unwrap_or(Action::Run(command)))
            .collect()
    }

    fn update(
        &mut self,
        keymap: &Keymap,
        shortcuts: &[(String, String)],
        enabled: impl Fn(Command) -> bool,
    ) {
        if self.shortcuts != shortcuts {
            for (item, command) in &self.items {
                let _ = item.set_accelerator(keymap.get(*command).and_then(accelerator));
            }
            self.shortcuts = shortcuts.to_vec();
        }
        for ((item, command), was) in self.items.iter().zip(&mut self.enabled) {
            // Text fields need the edit items even when the document has nothing to undo.
            let now = as_input(*command).is_some() || enabled(*command);
            if now != *was {
                item.set_enabled(now);
                *was = now;
            }
        }
    }
}

impl PilcrowApp {
    /// Opens files from Finder and runs what was picked in the menu bar.
    pub(super) fn mac_menu_frame(&mut self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        for path in pilcrow_macos::take_opened_files() {
            self.open_path(&path);
        }
        if pilcrow_macos::take_quit_request() {
            commands.push(Command::Quit);
        }
        let language = self.settings.language;
        let mut menu = match self.mac_menu.take() {
            Some(menu) if menu.language == language => menu,
            _ => {
                let ctx_for_files = ctx.clone();
                pilcrow_macos::set_waker(move || ctx_for_files.request_repaint());
                MacMenu::new(ctx, language, &self.keymap)
            }
        };
        for action in menu.poll() {
            match action {
                Action::Run(command) if self.dialog.is_none() || command == Command::Quit => {
                    commands.push(command);
                }
                Action::Run(_) => {}
                Action::Input(event) => ctx.input_mut(|input| input.events.push(event)),
                Action::Paste => ctx.send_viewport_cmd(ViewportCommand::RequestPaste),
            }
        }
        menu.update(&self.keymap, &self.settings.shortcuts, |command| {
            self.enabled(command)
        });
        self.mac_menu = Some(menu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_shortcuts_to_accelerators() {
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        assert_eq!(
            accelerator(save),
            Some(Accelerator::new(MenuModifiers::META, Code::KeyS))
        );
        assert_eq!(code(Key::Num0), Some(Code::Digit0));
        assert_eq!(code(Key::F11), Some(Code::F11));
        assert_eq!(code(Key::OpenBracket), Some(Code::BracketLeft));
    }
}
