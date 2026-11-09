//! Opening, saving and closing documents, sessions, backups, changes by other programs,
//! export and the text font.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuneiedit_core::export;
use cuneiedit_core::font::TrueType;
use cuneiedit_core::highlight::{Language as Syntax, TokenKind};
use cuneiedit_core::{DiskStatus, Document, Encoding, SaveError};
use cuneiedit_editor::Palette;
use eframe::egui::{self, FontData, FontDefinitions, FontFamily, ViewportCommand};

use super::CuneiEditApp;
use super::dialogs::{Dialog, UnsavedChoice};
use crate::i18n::{Texts, fill};
use crate::session::{self, Session, SessionTab};
use crate::settings::{Flag, SaveBackup, config_file};
use crate::system;

/// Larger font files are not loaded.
const MAX_FONT_SIZE: u64 = 50 * 1024 * 1024;
const USER_FONT: &str = "cuneiedit-user-font";

pub enum FontError {
    Io(io::Error),
    TooLarge,
    NotAFont,
}

impl FontError {
    pub fn message(&self, texts: &Texts) -> String {
        let reason = match self {
            Self::Io(err) => err.to_string(),
            Self::TooLarge => texts.font_too_large.to_owned(),
            Self::NotAFont => texts.not_a_font.to_owned(),
        };
        fill(texts.font_failed, &[("error", &reason)])
    }
}

fn read_font(path: &Path) -> Result<Vec<u8>, FontError> {
    if fs::metadata(path).map_err(FontError::Io)?.len() > MAX_FONT_SIZE {
        return Err(FontError::TooLarge);
    }
    let bytes = fs::read(path).map_err(FontError::Io)?;
    // TrueType, OpenType (CFF), Apple TrueType and font collections.
    let magic: [&[u8]; 4] = [b"\0\x01\0\0", b"OTTO", b"true", b"ttcf"];
    if !magic.iter().any(|m| bytes.starts_with(m)) {
        return Err(FontError::NotAFont);
    }
    Ok(bytes)
}

/// Uses the font file at `path` for the text, or the built-in font for `None`. Other text
/// keeps the built-in fonts, which also cover characters the chosen font lacks.
pub fn apply_font(ctx: &egui::Context, path: Option<&Path>) -> Result<(), FontError> {
    let mut fonts = FontDefinitions::default();
    let result = path.map(read_font).transpose();
    if let Ok(Some(bytes)) = &result {
        fonts.font_data.insert(
            USER_FONT.to_owned(),
            Arc::new(FontData::from_owned(bytes.clone())),
        );
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, USER_FONT.to_owned());
    }
    ctx.set_fonts(fonts);
    result.map(drop)
}

/// RGB of a syntax color, for export.
fn rgb(palette: &Palette, kind: Option<TokenKind>) -> [u8; 3] {
    let color = palette.color(kind);
    [color.r(), color.g(), color.b()]
}

/// The text fonts in the order the editor uses them (the user's font first, then the
/// built-in ones), as far as they can be embedded in a PDF.
fn text_fonts(ctx: &egui::Context) -> Vec<TrueType> {
    ctx.fonts(|fonts| {
        let definitions = fonts.definitions();
        definitions
            .families
            .get(&FontFamily::Monospace)
            .into_iter()
            .flatten()
            .filter_map(|name| definitions.font_data.get(name))
            .filter_map(|data| TrueType::parse((*data.font).as_ref(), data.index))
            .collect()
    })
}

impl CuneiEditApp {
    /// Reopens the tabs of the last run, with unsaved text from backups.
    pub(super) fn restore_session(&mut self) {
        let session = Session::load();
        let mut active = None;
        for (index, saved) in session.tabs.iter().enumerate() {
            // Files that no longer exist are skipped silently.
            let Some(doc) = restore_document(saved) else {
                continue;
            };
            let number = if doc.path().is_none() {
                saved.untitled.max(1)
            } else {
                0
            };
            let tab_index = self.push_tab(doc, number);
            let tab = &mut self.tabs[tab_index];
            if let Some(syntax) = saved.syntax.as_deref().and_then(Syntax::from_name) {
                tab.highlighter.set_language(syntax);
            }
            tab.editor
                .set_cursor(saved.cursor.min(tab.doc.text().len_chars()));
            let lines = tab.doc.text().len_lines();
            let bookmarks = saved.bookmarks.iter().copied().filter(|&l| l < lines);
            tab.editor.set_bookmarks(&tab.doc, bookmarks);
            tab.read_only = saved.read_only;
            tab.pinned = saved.pinned;
            tab.color = saved.color;
            if index == session.active {
                active = Some(tab_index);
            }
        }
        if let Some(active) = active {
            self.activate(active);
        }
    }

    /// Folder for file dialogs: that of the active document, if any.
    pub(super) fn dialog_dir(&self) -> Option<PathBuf> {
        self.tabs[self.active]
            .doc
            .path()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    }

    pub(super) fn open_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        for path in dialog.pick_files().unwrap_or_default() {
            self.open_path(&path);
        }
    }

    pub(super) fn open_path(&mut self, path: &Path) {
        if let Err(err) = self.open_file(path) {
            self.settings.recent.retain(|recent| recent != path);
            self.persist();
            let name = path.display().to_string();
            let message = fill(
                self.texts().open_failed,
                &[("name", &name), ("error", &err.to_string())],
            );
            self.error(message);
        }
    }

    fn open_file(&mut self, path: &Path) -> io::Result<()> {
        let path = path.canonicalize()?;
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.doc.path() == Some(&path))
        {
            self.activate(index);
            return Ok(());
        }
        let doc = Document::open(&path)?;
        if self.tabs.len() == 1 && self.tabs[0].is_pristine() {
            self.tabs.clear();
        }
        self.push_tab(doc, 0);
        self.settings.add_recent(&path);
        self.persist();
        Ok(())
    }

    /// Saves the tab, asking for a path if it has none. Returns whether it was saved.
    pub(super) fn save(&mut self, index: usize) -> bool {
        let Some(path) = self.tabs[index].doc.path().map(Path::to_path_buf) else {
            return self.save_as(index);
        };
        self.backup_before_save(&path);
        let result = self.tabs[index].doc.save();
        self.finish_save(index, result)
    }

    /// Keeps a copy of the file that is about to be overwritten, if the user wants one.
    /// A failed backup does not stop saving.
    fn backup_before_save(&self, path: &Path) {
        if !path.is_file() {
            return;
        }
        match self.settings.save_backup {
            SaveBackup::None => {}
            SaveBackup::Simple => {
                let mut name = path.as_os_str().to_owned();
                name.push(".bak");
                let _ = fs::copy(path, PathBuf::from(name));
            }
            SaveBackup::Verbose => {
                let (Some(dir), Some(name)) = (config_file("saved"), path.file_name()) else {
                    return;
                };
                let stamp = jiff::Zoned::now().strftime("%Y-%m-%d_%H%M%S").to_string();
                let target = dir.join(format!("{}.{stamp}.bak", name.to_string_lossy()));
                if session::create_private_dir(&dir).is_ok() {
                    let _ = fs::copy(path, target);
                }
            }
        }
    }

    /// The active document as PDF pages with syntax colors, for printing on white paper.
    fn active_pdf(&self, ctx: &egui::Context) -> Vec<u8> {
        let tab = &self.tabs[self.active];
        let palette = Palette::from_visuals(&egui::Visuals::light());
        let colors = |kind| rgb(&palette, kind);
        let title = tab.title(self.texts());
        export::pdf(
            &tab.doc,
            tab.syntax(),
            &title,
            self.settings.tab_width,
            &colors,
            &text_fonts(ctx),
        )
    }

    /// Prints the active document: the system print dialog on macOS and Windows, elsewhere a
    /// PDF to save.
    pub(super) fn print(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        {
            let title = self.tabs[self.active].title(self.texts());
            if cuneiedit_macos::print_pdf(&self.active_pdf(ctx), &title) {
                return;
            }
        }
        #[cfg(windows)]
        {
            let texts = self.texts();
            let tab = &self.tabs[self.active];
            let title = tab.title(texts);
            let palette = Palette::from_visuals(&egui::Visuals::light());
            let layout = |size: cuneiedit_windows::PageSize| {
                export::pages(
                    &tab.doc,
                    tab.syntax(),
                    self.settings.tab_width,
                    size.columns,
                    size.rows,
                )
                .into_iter()
                .map(|page| {
                    page.into_iter()
                        .map(|row| {
                            row.into_iter()
                                .map(|(text, kind)| (text, rgb(&palette, kind)))
                                .collect()
                        })
                        .collect()
                })
                .collect()
            };
            let footer = |page: usize, total: usize| format!("{title} \u{2013} {page}/{total}");
            if let Err(err) = cuneiedit_windows::print(&title, layout, footer) {
                let message = fill(texts.print_failed, &[("error", &err)]);
                self.error(message);
            }
            let _ = ctx;
        }
        #[cfg(not(windows))]
        self.export(ctx, true);
    }

    /// Exports the active document with syntax colors as a PDF or HTML page.
    pub(super) fn export(&mut self, ctx: &egui::Context, pdf: bool) {
        let texts = self.texts();
        let tab = &self.tabs[self.active];
        let title = tab.title(texts);
        let extension = if pdf { "pdf" } else { "html" };
        let mut dialog = rfd::FileDialog::new()
            .set_file_name(format!("{title}.{extension}"))
            .add_filter(extension.to_uppercase(), &[extension]);
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        let bytes = if pdf {
            self.active_pdf(ctx)
        } else {
            let palette = self
                .palette()
                .unwrap_or_else(|| Palette::from_visuals(&ctx.style_of(ctx.theme()).visuals));
            let colors = |kind| rgb(&palette, kind);
            let background = palette.background;
            let background = [background.r(), background.g(), background.b()];
            export::html(&tab.doc, tab.syntax(), &title, &colors, background).into_bytes()
        };
        if let Err(err) = fs::write(&path, bytes) {
            let name = path.display().to_string();
            let message = fill(
                texts.save_failed,
                &[("name", &name), ("error", &err.to_string())],
            );
            self.error(message);
        }
    }

    fn save_dialog(&self, index: usize) -> Option<PathBuf> {
        let tab = &self.tabs[index];
        let mut dialog = rfd::FileDialog::new().set_file_name(tab.title(self.texts()));
        if let Some(dir) = tab.doc.path().and_then(Path::parent) {
            dialog = dialog.set_directory(dir);
        }
        dialog.save_file()
    }

    pub(super) fn save_as(&mut self, index: usize) -> bool {
        let Some(path) = self.save_dialog(index) else {
            return false;
        };
        let result = self.tabs[index].doc.save_as(path);
        self.finish_save(index, result)
    }

    pub(super) fn save_copy(&mut self) {
        let Some(path) = self.save_dialog(self.active) else {
            return;
        };
        if let Err(err) = self.tabs[self.active].doc.save_copy(&path) {
            let message = self.save_error_message(self.active, err);
            self.error(message);
        }
    }

    pub(super) fn save_all(&mut self) {
        for index in 0..self.tabs.len() {
            if self.tabs[index].doc.is_modified() && !self.save(index) {
                return;
            }
        }
    }

    fn finish_save(&mut self, index: usize, result: Result<(), SaveError>) -> bool {
        match result {
            Ok(()) => {
                let tab = &mut self.tabs[index];
                tab.history.mark_saved(&tab.doc);
                if let Some(path) = tab.doc.path().map(Path::to_path_buf) {
                    if tab.syntax() == Syntax::PLAIN_TEXT {
                        tab.highlighter.set_language(Syntax::from_path(&path));
                    }
                    self.settings.add_recent(&path);
                    self.persist();
                }
                true
            }
            Err(err) => {
                let message = self.save_error_message(index, err);
                self.error(message);
                false
            }
        }
    }

    fn save_error_message(&self, index: usize, err: SaveError) -> String {
        let texts = self.texts();
        let tab = &self.tabs[index];
        match err {
            SaveError::Encode(err) => {
                let text = tab.doc.text();
                let line = text.char_to_line(err.char_index);
                let column = err.char_index - text.line_to_char(line);
                fill(
                    texts.unencodable,
                    &[
                        ("char", &err.ch.to_string()),
                        ("line", &(line + 1).to_string()),
                        ("column", &(column + 1).to_string()),
                        ("encoding", tab.doc.encoding().label()),
                    ],
                )
            }
            err => fill(
                texts.save_failed,
                &[("name", &tab.title(texts)), ("error", &err.to_string())],
            ),
        }
    }

    /// Closes the tabs one by one, asking about unsaved changes.
    pub(super) fn close_tabs(&mut self, ctx: &egui::Context, ids: Vec<u64>) {
        self.close_queue.extend(ids);
        self.process_close_queue(ctx);
    }

    fn process_close_queue(&mut self, ctx: &egui::Context) {
        while let Some(&id) = self.close_queue.front() {
            let Some(index) = self.tab_index(id) else {
                self.close_queue.pop_front();
                continue;
            };
            if self.tabs[index].doc.is_modified() {
                self.activate(index);
                self.dialog = Some(Dialog::Unsaved { tab: id });
                return;
            }
            self.close_queue.pop_front();
            if !self.quitting {
                self.remove_tab(index);
            }
        }
        if self.quitting {
            self.allow_close = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    pub(super) fn resolve_unsaved(&mut self, ctx: &egui::Context, id: u64, choice: UnsavedChoice) {
        let Some(index) = self.tab_index(id) else {
            self.close_queue.pop_front();
            self.process_close_queue(ctx);
            return;
        };
        match choice {
            UnsavedChoice::Cancel => {
                self.close_queue.clear();
                self.quitting = false;
                super::update::cancel_relaunch();
                return;
            }
            UnsavedChoice::Save if !self.save(index) => {
                // Saving failed or was cancelled; the error dialog explains why.
                self.close_queue.clear();
                self.quitting = false;
                return;
            }
            UnsavedChoice::Save | UnsavedChoice::Discard => {
                self.close_queue.pop_front();
                if !self.quitting {
                    self.remove_tab(index);
                }
            }
        }
        self.process_close_queue(ctx);
    }

    /// Handles a close request of the window: remembers the session and either quits
    /// right away (unsaved text is in backups) or asks about each unsaved document.
    pub(super) fn request_quit(&mut self, ctx: &egui::Context) {
        let backed_up = self.save_session_state();
        if backed_up || !self.tabs.iter().any(|tab| tab.doc.is_modified()) {
            return;
        }
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        if self.dialog.is_none() && !self.quitting {
            self.quitting = true;
            self.close_queue = self.tabs.iter().map(|tab| tab.id).collect();
            self.process_close_queue(ctx);
        }
    }

    /// Writes `session.ini` and backups of unsaved documents. Returns whether every
    /// unsaved document is in a backup.
    pub(super) fn save_session_state(&mut self) -> bool {
        let backup = self.settings.get(Flag::Backup);
        let mut session = Session::default();
        let mut complete = backup;
        let mut keep = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if tab.is_pristine() {
                continue;
            }
            let mut saved = SessionTab {
                path: tab.doc.path().map(Path::to_path_buf),
                encoding: Some(tab.doc.encoding()),
                line_ending: Some(tab.doc.line_ending()),
                syntax: Some(tab.syntax().name().to_owned()),
                untitled: tab.untitled_number,
                cursor: tab.editor.cursor(),
                bookmarks: tab.editor.bookmarks().iter().copied().collect(),
                read_only: tab.read_only,
                pinned: tab.pinned,
                color: tab.color,
                backup: None,
            };
            if backup && tab.doc.is_modified() {
                let name = session::backup_name(tab.id);
                let current = tab.backed_up == Some(tab.doc.revision());
                if current || session::write_backup(&name, &tab.doc.text().to_string()).is_ok() {
                    tab.backed_up = Some(tab.doc.revision());
                    saved.backup = Some(name.clone());
                    keep.push(name);
                } else {
                    complete = false;
                }
            }
            if saved.path.is_none() && saved.backup.is_none() {
                continue;
            }
            if index == self.active {
                session.active = session.tabs.len();
            }
            session.tabs.push(saved);
        }
        if session.save().is_err() {
            complete = false;
        }
        session::remove_stale_backups(&keep);
        complete
    }

    /// Updates backups if any unsaved document changed since the last one.
    pub(super) fn backup_if_needed(&mut self) {
        let outdated = self
            .tabs
            .iter()
            .any(|tab| tab.doc.is_modified() && tab.backed_up != Some(tab.doc.revision()));
        if self.settings.get(Flag::Backup) && outdated {
            self.save_session_state();
        }
    }

    pub(super) fn load_session_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("CuneiEdit", &["session"])
            .pick_file()
        else {
            return;
        };
        match Session::load_file(&path) {
            Ok(files) => {
                for file in files {
                    self.open_path(&file);
                }
            }
            Err(err) => {
                let name = path.display().to_string();
                let message = fill(
                    self.texts().open_failed,
                    &[("name", &name), ("error", &err.to_string())],
                );
                self.error(message);
            }
        }
    }

    pub(super) fn save_session_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("CuneiEdit", &["session"])
            .set_file_name("cuneiedit.session")
            .save_file()
        else {
            return;
        };
        let files: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter_map(|tab| tab.doc.path().map(Path::to_path_buf))
            .collect();
        if let Err(err) = Session::save_file(&path, &files) {
            let name = path.display().to_string();
            let message = fill(
                self.texts().save_failed,
                &[("name", &name), ("error", &err.to_string())],
            );
            self.error(message);
        }
    }

    /// Renames the file of a tab within its folder.
    pub(super) fn rename_tab(&mut self, id: u64, name: &str) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        let name = name.trim();
        let texts = self.texts();
        let tab = &mut self.tabs[index];
        let Some(dir) = tab.doc.path().and_then(Path::parent) else {
            return;
        };
        let valid = !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\']);
        let result = if valid {
            tab.doc.rename(dir.join(name))
        } else {
            Err(io::ErrorKind::InvalidInput.into())
        };
        match result {
            Ok(()) => {
                let path = tab.doc.path().map(Path::to_path_buf);
                if let Some(path) = path {
                    tab.highlighter.set_language(Syntax::from_path(&path));
                    self.settings.add_recent(&path);
                    self.persist();
                }
            }
            Err(err) => {
                let message = fill(
                    texts.rename_failed,
                    &[("name", &tab.title(texts)), ("error", &err.to_string())],
                );
                self.error(message);
            }
        }
    }

    pub(super) fn trash_tab(&mut self, id: u64) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        let Some(path) = self.tabs[index].doc.path().map(Path::to_path_buf) else {
            return;
        };
        match system::move_to_trash(&path) {
            Ok(_) => {
                self.remove_tab(index);
                self.closed.retain(|closed| *closed != path);
                self.settings.recent.retain(|recent| *recent != path);
                self.persist();
            }
            Err(err) => {
                let texts = self.texts();
                let message = fill(
                    texts.trash_failed,
                    &[
                        ("name", &self.tabs[index].title(texts)),
                        ("error", &err.to_string()),
                    ],
                );
                self.error(message);
            }
        }
    }

    pub(super) fn reveal(&mut self) {
        let Some(path) = self.tabs[self.active].doc.path().map(Path::to_path_buf) else {
            return;
        };
        if let Err(err) = system::reveal(&path) {
            let name = path.display().to_string();
            let message = fill(
                self.texts().open_failed,
                &[("name", &name), ("error", &err.to_string())],
            );
            self.error(message);
        }
    }

    /// Reloads the file of a tab, optionally reading it in a given encoding.
    pub(super) fn reload_tab(&mut self, index: usize, encoding: Option<Encoding>) {
        let texts = self.texts();
        let tab = &mut self.tabs[index];
        match tab.doc.reload(encoding) {
            Ok(()) => {
                tab.history.reset(&tab.doc);
                if tab.monitoring {
                    tab.editor.set_cursor(tab.doc.text().len_chars());
                }
            }
            Err(err) => {
                let message = fill(
                    texts.open_failed,
                    &[("name", &tab.title(texts)), ("error", &err.to_string())],
                );
                tab.doc.acknowledge_disk_change();
                self.error(message);
            }
        }
    }

    /// Looks for files changed or deleted by other programs.
    pub(super) fn check_disk(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        let ask = self.settings.get(Flag::AskBeforeReload);
        for index in 0..self.tabs.len() {
            let tab = &self.tabs[index];
            match tab.doc.disk_status() {
                DiskStatus::Unchanged => {}
                DiskStatus::Modified if tab.monitoring || (!ask && !tab.doc.is_modified()) => {
                    self.reload_tab(index, None);
                }
                status => {
                    self.dialog = Some(Dialog::DiskChanged {
                        tab: tab.id,
                        deleted: status == DiskStatus::Deleted,
                    });
                    return;
                }
            }
        }
    }

    /// `accept` means "Reload" for a changed file and "Keep" for a deleted one.
    pub(super) fn resolve_disk_change(
        &mut self,
        ctx: &egui::Context,
        id: u64,
        deleted: bool,
        accept: bool,
    ) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        match (deleted, accept) {
            (false, true) => self.reload_tab(index, None),
            (true, false) => self.close_tabs(ctx, vec![id]),
            // Keep the current text; a deleted file's text now counts as unsaved.
            _ => self.tabs[index].doc.acknowledge_disk_change(),
        }
    }
}

fn restore_document(saved: &SessionTab) -> Option<Document> {
    let mut doc = match &saved.backup {
        Some(name) => {
            let text = session::read_backup(name).ok()?;
            let mut doc = Document::from_bytes(text.into_bytes());
            doc.set_path(saved.path.clone());
            doc
        }
        None => Document::open(saved.path.as_ref()?).ok()?,
    };
    if saved.backup.is_some() {
        if let Some(encoding) = saved.encoding {
            doc.set_encoding(encoding);
        }
        if let Some(line_ending) = saved.line_ending {
            doc.set_line_ending(line_ending);
        }
        doc.mark_modified();
    }
    Some(doc)
}
