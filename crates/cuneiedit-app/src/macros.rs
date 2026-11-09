//! Saved macros, stored in `macros.ini` in the config folder. Only keyboard input, copy,
//! cut and paste are stored, never code.

use std::{fs, io};

use eframe::egui::{Event, Key, Modifiers};

use crate::settings::{config_file, write_atomically};

const FILE: &str = "macros.ini";
const MAX_MACROS: usize = 100;
const MAX_EVENTS: usize = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub struct SavedMacro {
    pub name: String,
    pub events: Vec<Event>,
}

pub fn load() -> Vec<SavedMacro> {
    config_file(FILE)
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

pub fn save(macros: &[SavedMacro]) -> io::Result<()> {
    match config_file(FILE) {
        Some(path) => write_atomically(&path, &serialize(macros)),
        None => Ok(()),
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

const MODIFIER_NAMES: [&str; 5] = ["alt", "ctrl", "shift", "mac_cmd", "command"];

fn modifier_flags(modifiers: &mut Modifiers) -> [&mut bool; 5] {
    [
        &mut modifiers.alt,
        &mut modifiers.ctrl,
        &mut modifiers.shift,
        &mut modifiers.mac_cmd,
        &mut modifiers.command,
    ]
}

fn format_key(key: Key, mut modifiers: Modifiers) -> String {
    let mut parts: Vec<&str> = MODIFIER_NAMES
        .iter()
        .zip(modifier_flags(&mut modifiers))
        .filter(|(_, on)| **on)
        .map(|(name, _)| *name)
        .collect();
    parts.push(key.name());
    parts.join("+")
}

fn parse_key(text: &str) -> Option<Event> {
    let mut modifiers = Modifiers::NONE;
    let mut parts: Vec<&str> = text.split('+').collect();
    // The key itself may be "+" (an empty last part).
    let name = match parts.pop()? {
        "" if text.ends_with('+') => {
            parts.pop();
            "+"
        }
        name => name,
    };
    for part in parts {
        let index = MODIFIER_NAMES.iter().position(|n| *n == part)?;
        *modifier_flags(&mut modifiers).into_iter().nth(index)? = true;
    }
    Some(Event::Key {
        key: Key::from_name(name)?,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    })
}

fn parse(text: &str) -> Vec<SavedMacro> {
    let mut macros: Vec<SavedMacro> = Vec::new();
    for line in text.lines() {
        if line == "[macro]" {
            if macros.len() >= MAX_MACROS {
                break;
            }
            macros.push(SavedMacro {
                name: String::new(),
                events: Vec::new(),
            });
            continue;
        }
        let Some(current) = macros.last_mut() else {
            continue;
        };
        if current.events.len() >= MAX_EVENTS {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        let event = match key {
            "name" => {
                current.name = unescape(value);
                None
            }
            "text" => Some(Event::Text(unescape(value))),
            "paste" => Some(Event::Paste(unescape(value))),
            "copy" => Some(Event::Copy),
            "cut" => Some(Event::Cut),
            "key" => parse_key(value),
            _ => None,
        };
        current.events.extend(event);
    }
    macros.retain(|m| !m.name.is_empty() && !m.events.is_empty());
    macros
}

fn serialize(macros: &[SavedMacro]) -> String {
    let mut out = String::new();
    for saved in macros {
        out.push_str("[macro]\n");
        out.push_str(&format!("name={}\n", escape(&saved.name)));
        for event in &saved.events {
            let line = match event {
                Event::Text(text) => format!("text={}", escape(text)),
                Event::Ime(eframe::egui::ImeEvent::Commit(text)) => {
                    format!("text={}", escape(text))
                }
                Event::Paste(text) => format!("paste={}", escape(text)),
                Event::Copy => "copy".to_owned(),
                Event::Cut => "cut".to_owned(),
                Event::Key { key, modifiers, .. } => {
                    format!("key={}", format_key(*key, *modifiers))
                }
                _ => continue,
            };
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let key = |key, modifiers| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let macros = vec![SavedMacro {
            name: "Fix = lines\n".into(),
            events: vec![
                Event::Text("a\\b\tc".into()),
                key(Key::ArrowDown, Modifiers::SHIFT | Modifiers::MAC_CMD),
                key(Key::Plus, Modifiers::COMMAND),
                Event::Copy,
                Event::Cut,
                Event::Paste("x\r\ny".into()),
            ],
        }];
        assert_eq!(parse(&serialize(&macros)), macros);
    }

    #[test]
    fn skips_garbage() {
        let parsed = parse("text=orphan\n[macro]\nname=a\nkey=hyper+Q\nkey=Nope\nfoo\n[macro]\n");
        assert!(parsed.is_empty());
        let parsed = parse("[macro]\nname=b\ntext=x\n");
        assert_eq!(parsed[0].events, vec![Event::Text("x".into())]);
    }
}
