//! Open tabs remembered between runs, including unsaved text ("backups"), and session files
//! the user saves explicitly.
//!
//! `session.ini` and the backup folder live next to `settings.ini`. Backup file names are
//! validated, so an edited `session.ini` cannot make Pilcrow read or delete other files.

use std::path::{Path, PathBuf};
use std::{fs, io};

use pilcrow_core::{Encoding, LineEnding};

use crate::settings::{config_file, safe_path, write_atomically};

const MAX_TABS: usize = 500;
const BACKUP_DIR: &str = "backup";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionTab {
    pub path: Option<PathBuf>,
    /// Backup file with unsaved text, see [`backup_name`].
    pub backup: Option<String>,
    pub encoding: Option<Encoding>,
    pub line_ending: Option<LineEnding>,
    pub syntax: Option<String>,
    pub untitled: usize,
    pub cursor: usize,
    pub bookmarks: Vec<usize>,
    pub read_only: bool,
    pub pinned: bool,
    pub color: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
}

impl Session {
    pub fn load() -> Self {
        config_file("session.ini")
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        match config_file("session.ini") {
            Some(path) => write_atomically(&path, &self.serialize()),
            None => Ok(()),
        }
    }

    /// Reads a session file saved by the user. Only file paths are used from it.
    pub fn load_file(path: &Path) -> io::Result<Vec<PathBuf>> {
        let session = Self::parse(&fs::read_to_string(path)?);
        Ok(session
            .tabs
            .into_iter()
            .filter_map(|tab| tab.path)
            .collect())
    }

    pub fn save_file(path: &Path, files: &[PathBuf]) -> io::Result<()> {
        let session = Self {
            tabs: files
                .iter()
                .map(|path| SessionTab {
                    path: Some(path.clone()),
                    ..SessionTab::default()
                })
                .collect(),
            active: 0,
        };
        write_atomically(path, &session.serialize())
    }

    pub fn parse(text: &str) -> Self {
        let mut session = Self::default();
        for line in text.lines() {
            if line == "[tab]" {
                if session.tabs.len() >= MAX_TABS {
                    break;
                }
                session.tabs.push(SessionTab::default());
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key == "active" {
                session.active = value.parse().unwrap_or(0);
                continue;
            }
            let Some(tab) = session.tabs.last_mut() else {
                continue;
            };
            match key {
                "path" if !value.is_empty() => tab.path = Some(PathBuf::from(value)),
                "backup" if is_backup_name(value) => tab.backup = Some(value.to_owned()),
                "encoding" => tab.encoding = Encoding::from_label(value),
                "line_ending" => {
                    tab.line_ending = LineEnding::ALL.into_iter().find(|e| e.label() == value);
                }
                "syntax" => tab.syntax = Some(value.to_owned()),
                "untitled" => tab.untitled = value.parse().unwrap_or(0),
                "cursor" => tab.cursor = value.parse().unwrap_or(0),
                "bookmarks" => {
                    tab.bookmarks = value
                        .split(',')
                        .filter_map(|n| n.parse().ok())
                        .take(100_000)
                        .collect();
                }
                "read_only" => tab.read_only = value == "true",
                "pinned" => tab.pinned = value == "true",
                "color" => tab.color = value.parse().ok().filter(|&c: &usize| c < 5),
                _ => {}
            }
        }
        session
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("active={}\n", self.active);
        for tab in &self.tabs {
            out.push_str("[tab]\n");
            if let Some(path) = tab.path.as_deref().and_then(safe_path) {
                out.push_str(&format!("path={path}\n"));
            }
            if let Some(backup) = &tab.backup {
                out.push_str(&format!("backup={backup}\n"));
            }
            if let Some(encoding) = tab.encoding {
                out.push_str(&format!("encoding={}\n", encoding.label()));
            }
            if let Some(line_ending) = tab.line_ending {
                out.push_str(&format!("line_ending={}\n", line_ending.label()));
            }
            if let Some(syntax) = &tab.syntax {
                out.push_str(&format!("syntax={syntax}\n"));
            }
            if tab.untitled > 0 {
                out.push_str(&format!("untitled={}\n", tab.untitled));
            }
            if tab.cursor > 0 {
                out.push_str(&format!("cursor={}\n", tab.cursor));
            }
            if !tab.bookmarks.is_empty() {
                let lines: Vec<String> = tab.bookmarks.iter().map(usize::to_string).collect();
                out.push_str(&format!("bookmarks={}\n", lines.join(",")));
            }
            if tab.read_only {
                out.push_str("read_only=true\n");
            }
            if tab.pinned {
                out.push_str("pinned=true\n");
            }
            if let Some(color) = tab.color {
                out.push_str(&format!("color={color}\n"));
            }
        }
        out
    }
}

/// Name of the backup file for a tab.
pub fn backup_name(tab_id: u64) -> String {
    format!("tab-{tab_id}.txt")
}

fn is_backup_name(name: &str) -> bool {
    name.strip_prefix("tab-")
        .and_then(|rest| rest.strip_suffix(".txt"))
        .is_some_and(|id| {
            !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
        })
}

fn backup_path(name: &str) -> io::Result<PathBuf> {
    if !is_backup_name(name) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    config_file(BACKUP_DIR)
        .map(|dir| dir.join(name))
        .ok_or_else(|| io::ErrorKind::NotFound.into())
}

/// Writes unsaved text as UTF-8. The folder is only accessible to the user.
pub fn write_backup(name: &str, text: &str) -> io::Result<()> {
    let path = backup_path(name)?;
    if let Some(dir) = path.parent() {
        create_private_dir(dir)?;
    }
    write_atomically(&path, text)
}

pub fn read_backup(name: &str) -> io::Result<String> {
    fs::read_to_string(backup_path(name)?)
}

/// Deletes backup files not listed in `keep`. Other files in the folder are left alone.
pub fn remove_stale_backups(keep: &[String]) {
    let Some(dir) = config_file(BACKUP_DIR) else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_backup_name(&name) && !keep.contains(&name) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Creates `dir` so that only the user can read it.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    fs::create_dir_all(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let session = Session {
            tabs: vec![
                SessionTab {
                    path: Some("/a/b.txt".into()),
                    cursor: 12,
                    bookmarks: vec![1, 5],
                    syntax: Some("Rust".into()),
                    read_only: true,
                    pinned: true,
                    color: Some(2),
                    ..SessionTab::default()
                },
                SessionTab {
                    backup: Some(backup_name(7)),
                    untitled: 3,
                    encoding: Some(Encoding::Utf16Le),
                    line_ending: Some(LineEnding::CrLf),
                    ..SessionTab::default()
                },
            ],
            active: 1,
        };
        assert_eq!(Session::parse(&session.serialize()), session);
    }

    #[test]
    fn rejects_unsafe_backup_names() {
        for name in [
            "../settings.ini",
            "tab-.txt",
            "tab-1.txt/../../x",
            "x.txt",
            "tab-1a.txt",
        ] {
            assert!(!is_backup_name(name), "{name}");
            assert!(backup_path(name).is_err());
        }
        assert!(is_backup_name("tab-42.txt"));
        let parsed = Session::parse("[tab]\nbackup=../../etc/passwd\ncolor=9");
        assert_eq!(parsed.tabs[0].backup, None);
        assert_eq!(parsed.tabs[0].color, None);
    }

    #[test]
    fn user_session_files_contain_only_paths() {
        let path = std::env::temp_dir().join(format!("pilcrow-session-{}.ini", std::process::id()));
        let files = vec![PathBuf::from("/x/one.rs"), PathBuf::from("/y/two.md")];
        Session::save_file(&path, &files).unwrap();
        assert_eq!(Session::load_file(&path).unwrap(), files);
        let _ = fs::remove_file(path);
    }
}
