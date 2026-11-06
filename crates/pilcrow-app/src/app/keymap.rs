//! Keyboard shortcuts in effect: the defaults from `command.rs` with the user's changes.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

use super::command::{CUSTOMIZABLE, Command, SHORTCUTS};

pub struct Keymap {
    /// More modifiers first, so a shortcut is not shadowed by one with fewer.
    entries: Vec<(KeyboardShortcut, Command)>,
}

/// Stable name of a command in the settings file.
pub fn command_id(command: Command) -> String {
    format!("{command:?}")
}

/// `cmd+shift+K`; `cmd` is ⌘ on macOS and Ctrl elsewhere.
pub fn format(shortcut: &KeyboardShortcut) -> String {
    let m = shortcut.modifiers;
    let mut parts = Vec::new();
    if m.command || m.mac_cmd {
        parts.push("cmd");
    }
    if m.ctrl && !m.command {
        parts.push("ctrl");
    }
    if m.alt {
        parts.push("alt");
    }
    if m.shift {
        parts.push("shift");
    }
    parts.push(shortcut.logical_key.name());
    parts.join("+")
}

pub fn parse(text: &str) -> Option<KeyboardShortcut> {
    let (mods, name) = match text.rsplit_once('+') {
        Some((mods, "")) => (mods.strip_suffix('+')?, "+"),
        Some((mods, name)) => (mods, name),
        None => ("", text),
    };
    let mut modifiers = Modifiers::NONE;
    for part in mods.split('+').filter(|p| !p.is_empty()) {
        match part {
            "cmd" => modifiers |= Modifiers::COMMAND,
            "ctrl" => modifiers |= Modifiers::CTRL,
            "alt" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            _ => return None,
        }
    }
    Some(KeyboardShortcut::new(modifiers, Key::from_name(name)?))
}

/// The shortcut for a pressed key, as it would be stored: ⌘ (or Ctrl outside macOS) is
/// `cmd`, Ctrl on macOS is `ctrl`.
pub fn from_press(key: Key, pressed: Modifiers, mac: bool) -> KeyboardShortcut {
    let mut modifiers = Modifiers::NONE;
    if pressed.command || pressed.mac_cmd {
        modifiers |= Modifiers::COMMAND;
    }
    if mac && pressed.ctrl {
        modifiers |= Modifiers::CTRL;
    }
    if pressed.alt {
        modifiers |= Modifiers::ALT;
    }
    if pressed.shift {
        modifiers |= Modifiers::SHIFT;
    }
    KeyboardShortcut::new(modifiers, key)
}

fn modifier_count(shortcut: &KeyboardShortcut) -> usize {
    let m = shortcut.modifiers;
    [m.alt, m.ctrl, m.shift, m.command || m.mac_cmd]
        .into_iter()
        .filter(|&on| on)
        .count()
}

impl Keymap {
    /// The defaults with `changes` (command id, shortcut text or empty to remove) applied.
    pub fn new(changes: &[(String, String)]) -> Self {
        let changed = |command: Command| {
            let id = command_id(command);
            changes
                .iter()
                .find(|(changed, _)| *changed == id)
                .map(|(_, text)| parse(text))
        };
        let mut entries: Vec<(KeyboardShortcut, Command)> = SHORTCUTS
            .iter()
            .filter(|(_, command)| changed(*command).is_none())
            .copied()
            .collect();
        for &command in CUSTOMIZABLE.iter().chain(SHORTCUTS.iter().map(|(_, c)| c)) {
            if let Some(Some(shortcut)) = changed(command)
                && !entries.iter().any(|(_, c)| *c == command)
            {
                // A shortcut moved to this command is taken from the others.
                entries.retain(|(s, _)| *s != shortcut);
                entries.push((shortcut, command));
            }
        }
        entries.sort_by_key(|(shortcut, _)| std::cmp::Reverse(modifier_count(shortcut)));
        Self { entries }
    }

    pub fn entries(&self) -> &[(KeyboardShortcut, Command)] {
        &self.entries
    }

    pub fn get(&self, command: Command) -> Option<KeyboardShortcut> {
        let command = match command {
            Command::StopRecording => Command::StartRecording,
            other => other,
        };
        self.entries
            .iter()
            .find(|(_, c)| *c == command)
            .map(|(shortcut, _)| *shortcut)
            .or_else(|| command.editor_shortcut())
    }

    /// The command that `shortcut` triggers, if any.
    pub fn command_for(&self, shortcut: &KeyboardShortcut) -> Option<Command> {
        self.entries
            .iter()
            .find(|(s, _)| s == shortcut)
            .map(|(_, command)| *command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_parses() {
        let shortcut = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
        assert_eq!(format(&shortcut), "cmd+shift+K");
        assert_eq!(parse("cmd+shift+K"), Some(shortcut));
        let plus = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
        assert_eq!(parse(&format(&plus)), Some(plus));
        assert_eq!(parse("hyper+K"), None);
        assert_eq!(parse("cmd+Nope"), None);
        let pressed = Modifiers {
            mac_cmd: true,
            command: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            from_press(Key::S, pressed, true),
            KeyboardShortcut::new(Modifiers::COMMAND, Key::S)
        );
    }

    #[test]
    fn applies_changes() {
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let keymap = Keymap::new(&[]);
        assert_eq!(keymap.get(Command::Save), Some(save));
        assert_eq!(keymap.command_for(&save), Some(Command::Save));

        let changes = vec![
            (command_id(Command::Save), String::new()),
            (command_id(Command::Summary), "cmd+S".into()),
        ];
        let keymap = Keymap::new(&changes);
        assert_eq!(keymap.get(Command::Save), None);
        assert_eq!(keymap.command_for(&save), Some(Command::Summary));
    }

    #[test]
    fn more_modifiers_come_first() {
        let keymap = Keymap::new(&[(command_id(Command::Summary), "alt+F2".into())]);
        let entries = keymap.entries();
        for (i, (later, _)) in entries.iter().enumerate() {
            for (earlier, _) in &entries[..i] {
                assert!(modifier_count(earlier) >= modifier_count(later));
            }
        }
    }
}
