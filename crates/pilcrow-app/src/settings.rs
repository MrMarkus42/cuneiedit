//! Persistent settings, stored as `key=value` lines in the platform's config directory.

use std::path::{Path, PathBuf};
use std::{env, fs, io};

use eframe::egui::ThemePreference;
use pilcrow_core::{Encoding, LineEnding};

use crate::i18n::Language;

pub const MAX_RECENT: usize = 10;
const MAX_SHORTCUTS: usize = 1_000;
pub const DEFAULT_FONT_SIZE: f32 = 14.0;
const FONT_SIZES: std::ops::RangeInclusive<f32> = 6.0..=72.0;

/// On/off options. Each has a settings key and a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flag {
    IndentWithSpaces,
    ShowWhitespace,
    ShowLineEndings,
    ShowWrapSymbol,
    IndentGuides,
    WordWrap,
    AutoClose,
    EdgeLine,
    /// Restore unsaved documents on start instead of asking on quit.
    Backup,
    ChangeHistory,
    AutoComplete,
    /// Ask before reloading files changed by another program.
    AskBeforeReload,
    FunctionList,
    DocumentList,
    ClipboardHistory,
    CharacterPanel,
    /// Highlight other occurrences of a selected word.
    SmartHighlight,
    /// Fold markers in the gutter.
    Folding,
    DocumentMap,
    FileTree,
    Toolbar,
}

impl Flag {
    pub const ALL: [Self; 21] = [
        Self::IndentWithSpaces,
        Self::ShowWhitespace,
        Self::ShowLineEndings,
        Self::ShowWrapSymbol,
        Self::IndentGuides,
        Self::WordWrap,
        Self::AutoClose,
        Self::EdgeLine,
        Self::Backup,
        Self::ChangeHistory,
        Self::AutoComplete,
        Self::AskBeforeReload,
        Self::FunctionList,
        Self::DocumentList,
        Self::ClipboardHistory,
        Self::CharacterPanel,
        Self::SmartHighlight,
        Self::Folding,
        Self::DocumentMap,
        Self::FileTree,
        Self::Toolbar,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::IndentWithSpaces => "indent_with_spaces",
            Self::ShowWhitespace => "show_whitespace",
            Self::ShowLineEndings => "show_line_endings",
            Self::ShowWrapSymbol => "show_wrap_symbol",
            Self::IndentGuides => "indent_guides",
            Self::WordWrap => "word_wrap",
            Self::AutoClose => "auto_close",
            Self::EdgeLine => "edge_line",
            Self::Backup => "backup",
            Self::ChangeHistory => "change_history",
            Self::AutoComplete => "auto_complete",
            Self::AskBeforeReload => "ask_before_reload",
            Self::FunctionList => "function_list",
            Self::DocumentList => "document_list",
            Self::ClipboardHistory => "clipboard_history",
            Self::CharacterPanel => "character_panel",
            Self::SmartHighlight => "smart_highlight",
            Self::Folding => "folding",
            Self::DocumentMap => "document_map",
            Self::FileTree => "file_tree",
            Self::Toolbar => "toolbar",
        }
    }

    fn default(self) -> bool {
        matches!(
            self,
            Self::Backup
                | Self::ChangeHistory
                | Self::AskBeforeReload
                | Self::IndentGuides
                | Self::SmartHighlight
                | Self::Folding
                | Self::Toolbar
        )
    }
}

/// Copies kept when a file is saved over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SaveBackup {
    #[default]
    None,
    /// `name.bak` next to the file.
    Simple,
    /// A copy with a timestamp in the config folder.
    Verbose,
}

impl SaveBackup {
    pub const ALL: [Self; 3] = [Self::None, Self::Simple, Self::Verbose];

    fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Simple => "simple",
            Self::Verbose => "verbose",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub language: Language,
    pub theme: ThemePreference,
    pub font_size: f32,
    pub tab_width: usize,
    pub edge_column: usize,
    flags: Vec<(Flag, bool)>,
    pub recent: Vec<PathBuf>,
    /// Format for "insert date and time" (strftime syntax).
    pub date_format: String,
    pub new_line_ending: LineEnding,
    pub new_encoding: Encoding,
    /// Name of a color theme for the text; empty follows the appearance.
    pub color_theme: String,
    /// Font file for the text; `None` is the built-in font.
    pub font_path: Option<PathBuf>,
    pub save_backup: SaveBackup,
    /// Changed keyboard shortcuts: command id and shortcut (empty if removed).
    pub shortcuts: Vec<(String, String)>,
    /// Folder shown in the file tree.
    pub workspace: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::from_system(),
            theme: ThemePreference::System,
            font_size: DEFAULT_FONT_SIZE,
            tab_width: 4,
            edge_column: 80,
            flags: Flag::ALL
                .iter()
                .map(|&flag| (flag, flag.default()))
                .collect(),
            recent: Vec::new(),
            date_format: "%Y-%m-%d %H:%M:%S".into(),
            new_line_ending: LineEnding::default(),
            new_encoding: Encoding::Utf8,
            color_theme: String::new(),
            font_path: None,
            save_backup: SaveBackup::None,
            shortcuts: Vec::new(),
            workspace: None,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        config_file("settings.ini")
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        match config_file("settings.ini") {
            Some(path) => write_atomically(&path, &self.serialize()),
            None => Ok(()),
        }
    }

    pub fn get(&self, flag: Flag) -> bool {
        self.flags
            .iter()
            .find(|(f, _)| *f == flag)
            .is_some_and(|(_, on)| *on)
    }

    pub fn set(&mut self, flag: Flag, on: bool) {
        if let Some(entry) = self.flags.iter_mut().find(|(f, _)| *f == flag) {
            entry.1 = on;
        }
    }

    pub fn toggle(&mut self, flag: Flag) {
        self.set(flag, !self.get(flag));
    }

    pub fn add_recent(&mut self, path: &Path) {
        self.recent.retain(|recent| recent != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    pub fn set_font_size(&mut self, size: f32) {
        self.font_size = size.clamp(*FONT_SIZES.start(), *FONT_SIZES.end());
    }

    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if let Some(&flag) = Flag::ALL.iter().find(|flag| flag.key() == key) {
                settings.set(flag, value == "true");
                continue;
            }
            match key {
                "language" => {
                    if let Some(language) = Language::from_code(value) {
                        settings.language = language;
                    }
                }
                "theme" => {
                    settings.theme = match value {
                        "light" => ThemePreference::Light,
                        "dark" => ThemePreference::Dark,
                        _ => ThemePreference::System,
                    }
                }
                "font_size" => {
                    if let Some(size) = value.parse::<f32>().ok().filter(|s| s.is_finite()) {
                        settings.set_font_size(size);
                    }
                }
                "tab_width" => {
                    if let Ok(width) = value.parse::<usize>() {
                        settings.tab_width = width.clamp(1, 16);
                    }
                }
                "edge_column" => {
                    if let Ok(column) = value.parse::<usize>() {
                        settings.edge_column = column.clamp(1, 1000);
                    }
                }
                "date_format" if !value.is_empty() => settings.date_format = value.to_owned(),
                "new_line_ending" => {
                    if let Some(&ending) = LineEnding::ALL.iter().find(|e| e.label() == value) {
                        settings.new_line_ending = ending;
                    }
                }
                "new_encoding" => {
                    if let Some(encoding) = Encoding::from_label(value) {
                        settings.new_encoding = encoding;
                    }
                }
                "recent" if settings.recent.len() < MAX_RECENT => {
                    settings.recent.push(PathBuf::from(value));
                }
                "color_theme" => settings.color_theme = value.to_owned(),
                "font_path" if !value.is_empty() => settings.font_path = Some(value.into()),
                "workspace" if !value.is_empty() => settings.workspace = Some(value.into()),
                "save_backup" => {
                    if let Some(&mode) = SaveBackup::ALL.iter().find(|m| m.key() == value) {
                        settings.save_backup = mode;
                    }
                }
                _ => {
                    if let Some(id) = key.strip_prefix("shortcut.")
                        && settings.shortcuts.len() < MAX_SHORTCUTS
                    {
                        settings.shortcuts.push((id.to_owned(), value.to_owned()));
                    }
                }
            }
        }
        settings
    }

    fn serialize(&self) -> String {
        let theme = match self.theme {
            ThemePreference::Light => "light",
            ThemePreference::Dark => "dark",
            ThemePreference::System => "system",
        };
        let mut lines = vec![
            format!("language={}", self.language.code()),
            format!("theme={theme}"),
            format!("font_size={}", self.font_size),
            format!("tab_width={}", self.tab_width),
            format!("edge_column={}", self.edge_column),
            format!("date_format={}", self.date_format.replace(['\n', '\r'], "")),
            format!("new_line_ending={}", self.new_line_ending.label()),
            format!("new_encoding={}", self.new_encoding.label()),
            format!("color_theme={}", self.color_theme.replace(['\n', '\r'], "")),
            format!("save_backup={}", self.save_backup.key()),
        ];
        if let Some(path) = self.font_path.as_deref().and_then(safe_path) {
            lines.push(format!("font_path={path}"));
        }
        if let Some(path) = self.workspace.as_deref().and_then(safe_path) {
            lines.push(format!("workspace={path}"));
        }
        lines.extend(
            self.shortcuts
                .iter()
                .filter(|(id, text)| !format!("{id}{text}").contains(['\n', '\r', '=']))
                .map(|(id, text)| format!("shortcut.{id}={text}")),
        );
        lines.extend(
            self.flags
                .iter()
                .map(|(flag, on)| format!("{}={on}", flag.key())),
        );
        lines.extend(
            self.recent
                .iter()
                .filter_map(|path| safe_path(path))
                .map(|path| format!("recent={path}")),
        );
        lines.join("\n") + "\n"
    }
}

/// `path` as a string, if it can be stored in a line-based file.
pub fn safe_path(path: &Path) -> Option<&str> {
    path.to_str().filter(|path| !path.contains(['\n', '\r']))
}

/// Writes `contents` via a temporary file and a rename, so a crash never leaves a
/// truncated file behind.
pub fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, contents)?;
    fs::rename(temp, path)
}

pub fn config_file(name: &str) -> Option<PathBuf> {
    Some(config_dir()?.join(name))
}

fn config_dir() -> Option<PathBuf> {
    let home = || {
        env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        Some(home()?.join("Library/Application Support/Pilcrow"))
    } else if cfg!(windows) {
        let app_data = env::var_os("APPDATA").filter(|a| !a.is_empty())?;
        Some(PathBuf::from(app_data).join("Pilcrow"))
    } else {
        let config = env::var_os("XDG_CONFIG_HOME")
            .filter(|c| !c.is_empty())
            .map(PathBuf::from)
            .or_else(|| Some(home()?.join(".config")))?;
        Some(config.join("pilcrow"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let mut settings = Settings {
            language: Language::German,
            theme: ThemePreference::Dark,
            font_size: 17.0,
            tab_width: 2,
            edge_column: 100,
            recent: vec!["/a/b.txt".into(), "/c=d.txt".into()],
            date_format: "%d.%m.%Y".into(),
            new_line_ending: LineEnding::CrLf,
            new_encoding: Encoding::from_label("ISO-8859-2").unwrap(),
            color_theme: "Nord".into(),
            font_path: Some("/fonts/a b.ttf".into()),
            save_backup: SaveBackup::Verbose,
            shortcuts: vec![
                ("Save".into(), "cmd+shift+X".into()),
                ("Line(Delete)".into(), String::new()),
            ],
            workspace: Some("/projects/x".into()),
            ..Settings::default()
        };
        settings.toggle(Flag::WordWrap);
        settings.toggle(Flag::Backup);
        assert_eq!(Settings::parse(&settings.serialize()), settings);
    }

    #[test]
    fn has_sensible_defaults() {
        let settings = Settings::default();
        assert!(settings.get(Flag::Backup));
        assert!(!settings.get(Flag::WordWrap));
    }

    #[test]
    fn ignores_invalid_values() {
        let parsed = Settings::parse(
            "font_size=NaN\ntab_width=999\nlanguage=xx\nnonsense\nfoo=bar\nnew_encoding=nope",
        );
        assert_eq!(parsed.font_size, DEFAULT_FONT_SIZE);
        assert_eq!(parsed.tab_width, 16);
        assert_eq!(parsed.language, Language::from_system());
        assert_eq!(parsed.new_encoding, Encoding::Utf8);
    }

    #[test]
    fn skips_paths_with_line_breaks() {
        let settings = Settings {
            recent: vec!["/ok".into(), "/bad\npath".into()],
            ..Settings::default()
        };
        assert_eq!(
            Settings::parse(&settings.serialize()).recent,
            vec![PathBuf::from("/ok")]
        );
    }

    #[test]
    fn keeps_recent_unique_and_short() {
        let mut settings = Settings::default();
        for i in 0..MAX_RECENT + 3 {
            settings.add_recent(Path::new(&format!("/f{i}")));
        }
        settings.add_recent(Path::new("/f5"));
        assert_eq!(settings.recent.len(), MAX_RECENT);
        assert_eq!(settings.recent[0], PathBuf::from("/f5"));
        assert_eq!(
            settings
                .recent
                .iter()
                .filter(|p| p.as_path() == Path::new("/f5"))
                .count(),
            1
        );
    }
}
