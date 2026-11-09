use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ropey::Rope;

use crate::history::History;
use crate::{EncodeError, Encoding, LineEnding};

/// Number of recent edits remembered for [`Document::first_change_since`].
const CHANGE_LOG_LEN: usize = 64;

/// A text document with its on-disk format and undo history.
///
/// Positions are char indices into [`Document::text`], whose lines always end in `\n`.
#[derive(Debug, Default)]
pub struct Document {
    text: Rope,
    path: Option<PathBuf>,
    encoding: Encoding,
    line_ending: LineEnding,
    history: History,
    saved_at: Option<u64>,
    format_changed: bool,
    revision: u64,
    /// `(revision after the edit, first changed char, unchanged chars after the edit)` of
    /// recent edits.
    changes: VecDeque<(u64, usize, usize)>,
    /// Modification time and size of the file when it was last read or written.
    disk_stamp: Option<(SystemTime, u64)>,
}

/// State of the file on disk compared to when it was last read or written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskStatus {
    Unchanged,
    Modified,
    Deleted,
}

fn disk_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty, unmodified document that will be saved in `encoding` with `line_ending`.
    pub fn with_format(encoding: Encoding, line_ending: LineEnding) -> Self {
        Self {
            encoding,
            line_ending,
            ..Self::default()
        }
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let (text, encoding) = Encoding::detect_and_decode(bytes);
        Self {
            line_ending: LineEnding::detect(&text),
            text: Rope::from_str(&LineEnding::normalize(&text)),
            encoding,
            ..Self::default()
        }
    }

    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let mut doc = Self::from_bytes(fs::read(&path)?);
        doc.disk_stamp = disk_stamp(&path);
        doc.path = Some(path);
        Ok(doc)
    }

    /// Replaces the text with the file on disk, as one undo step. With `encoding`, the file is
    /// read in that encoding instead of detecting it.
    pub fn reload(&mut self, encoding: Option<Encoding>) -> io::Result<()> {
        let path = self.path.clone().ok_or(io::ErrorKind::NotFound)?;
        let bytes = fs::read(&path)?;
        let (text, encoding) = match encoding {
            Some(encoding) => (encoding.decode(&bytes), encoding),
            None => Encoding::detect_and_decode(bytes),
        };
        let line_ending = LineEnding::detect(&text);
        let normalized = LineEnding::normalize(&text);
        if self.text != normalized.as_ref() {
            self.seal_undo_group();
            self.replace(0..self.text.len_chars(), &normalized);
            self.seal_undo_group();
        }
        self.encoding = encoding;
        self.line_ending = line_ending;
        self.saved_at = self.history.top_id();
        self.format_changed = false;
        self.disk_stamp = disk_stamp(&path);
        Ok(())
    }

    pub fn disk_status(&self) -> DiskStatus {
        let (Some(path), Some(stamp)) = (&self.path, self.disk_stamp) else {
            return DiskStatus::Unchanged;
        };
        match disk_stamp(path) {
            None if !path.exists() => DiskStatus::Deleted,
            Some(now) if now != stamp => DiskStatus::Modified,
            _ => DiskStatus::Unchanged,
        }
    }

    /// Accepts the current file on disk as known, e.g. after the user declined reloading.
    pub fn acknowledge_disk_change(&mut self) {
        self.disk_stamp = self.path.as_deref().and_then(disk_stamp);
        if self.disk_stamp.is_none() {
            // The file is gone; keeping the document means it has unsaved content.
            self.format_changed = true;
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let text = self.text.to_string();
        self.encoding
            .encode(&self.line_ending.apply(&text))
            // Re-encode without converted line endings so the error position matches the buffer.
            .map_err(|_| self.encoding.encode(&text).expect_err("same characters"))
    }

    pub fn save(&mut self) -> Result<(), SaveError> {
        let path = self.path.clone().ok_or(SaveError::NoPath)?;
        self.save_as(path)
    }

    pub fn save_as(&mut self, path: impl Into<PathBuf>) -> Result<(), SaveError> {
        let path = path.into();
        self.save_copy(&path)?;
        self.disk_stamp = disk_stamp(&path);
        self.path = Some(path);
        self.saved_at = self.history.top_id();
        self.format_changed = false;
        Ok(())
    }

    /// Writes the document to `path` without changing its own path or saved state.
    pub fn save_copy(&self, path: &Path) -> Result<(), SaveError> {
        let bytes = self.to_bytes().map_err(SaveError::Encode)?;
        fs::write(path, bytes).map_err(SaveError::Io)
    }

    /// Renames the file on disk and keeps editing it under the new name.
    pub fn rename(&mut self, new_path: impl Into<PathBuf>) -> io::Result<()> {
        let new_path = new_path.into();
        let old = self.path.as_ref().ok_or(io::ErrorKind::NotFound)?;
        if new_path.exists() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        fs::rename(old, &new_path)?;
        self.disk_stamp = disk_stamp(&new_path);
        self.path = Some(new_path);
        Ok(())
    }

    /// Sets the file path without touching the disk, e.g. when restoring a backup.
    pub fn set_path(&mut self, path: Option<PathBuf>) {
        self.disk_stamp = path.as_deref().and_then(disk_stamp);
        self.path = path;
    }

    /// Marks the document as having unsaved changes.
    pub fn mark_modified(&mut self) {
        self.format_changed = true;
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn file_name(&self) -> Option<String> {
        Some(
            self.path
                .as_ref()?
                .file_name()?
                .to_string_lossy()
                .into_owned(),
        )
    }

    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn set_encoding(&mut self, encoding: Encoding) {
        if self.encoding != encoding {
            self.encoding = encoding;
            self.format_changed = true;
        }
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        if self.line_ending != line_ending {
            self.line_ending = line_ending;
            self.format_changed = true;
        }
    }

    pub fn is_modified(&self) -> bool {
        self.format_changed || self.history.top_id() != self.saved_at
    }

    /// Increments on every change to the text.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Char index before which the text is unchanged since `revision`, or `None` if nothing
    /// changed. Returns `Some(0)` if `revision` is too old to know.
    pub fn first_change_since(&self, revision: u64) -> Option<usize> {
        self.unchanged_since(revision).map(|(head, _)| head)
    }

    /// Number of chars at the start and at the end of the text that are unchanged since
    /// `revision`, or `None` if nothing changed. Returns `Some((0, 0))` if `revision` is too
    /// old to know.
    pub fn unchanged_since(&self, revision: u64) -> Option<(usize, usize)> {
        if revision >= self.revision {
            return None;
        }
        let known = self
            .changes
            .front()
            .is_some_and(|&(oldest, ..)| oldest <= revision + 1);
        if !known {
            return Some((0, 0));
        }
        self.changes
            .iter()
            .filter(|&&(changed, ..)| changed > revision)
            .map(|&(_, head, tail)| (head, tail))
            .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)))
    }

    pub fn slice(&self, range: Range<usize>) -> String {
        self.text.slice(range).into()
    }

    /// Replaces `range` with `text` and returns the char index after the inserted text.
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> usize {
        let len = self.text.len_chars();
        let range = range.start.min(len)..range.end.min(len);
        let inserted = LineEnding::normalize(text).into_owned();
        let removed = self.slice(range.clone());
        if removed.is_empty() && inserted.is_empty() {
            return range.start;
        }
        self.splice(range.start, range.len(), &inserted);
        let end = range.start + inserted.chars().count();
        self.history
            .record(range.start, removed, inserted, self.saved_at);
        end
    }

    /// Replaces several non-overlapping ranges as one undo step. Returns, for every edit in
    /// the given order, the char index after its inserted text in the new document.
    pub fn replace_many(&mut self, edits: &[(Range<usize>, String)]) -> Vec<usize> {
        let len = self.text.len_chars();
        let mut order: Vec<usize> = (0..edits.len()).collect();
        order.sort_by_key(|&i| edits[i].0.start);
        let mut ranges: Vec<Range<usize>> = Vec::with_capacity(edits.len());
        let mut previous_end = 0;
        for &i in &order {
            let range = &edits[i].0;
            let start = range.start.min(len).max(previous_end);
            let end = range.end.min(len).max(start);
            previous_end = end;
            ranges.push(start..end);
        }
        let inserted: Vec<String> = order
            .iter()
            .map(|&i| LineEnding::normalize(&edits[i].1).into_owned())
            .collect();

        let mut ends = vec![0; edits.len()];
        let mut delta = 0isize;
        for (k, &i) in order.iter().enumerate() {
            let count = inserted[k].chars().count();
            ends[i] = ranges[k].start.saturating_add_signed(delta) + count;
            delta += count as isize - ranges[k].len() as isize;
        }

        self.history.seal();
        let mut first = true;
        for k in (0..order.len()).rev() {
            let range = ranges[k].clone();
            let removed = self.slice(range.clone());
            if removed.is_empty() && inserted[k].is_empty() {
                continue;
            }
            self.splice(range.start, range.len(), &inserted[k]);
            let text = inserted[k].clone();
            if std::mem::take(&mut first) {
                self.history
                    .record(range.start, removed, text, self.saved_at);
            } else {
                self.history.record_linked(range.start, removed, text);
            }
        }
        self.history.seal();
        ends
    }

    /// Ends the current undo group, e.g. after the cursor was moved.
    pub fn seal_undo_group(&mut self) {
        self.history.seal();
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Reverts the last edit and returns the cursor position after it.
    pub fn undo(&mut self) -> Option<usize> {
        let mut pos = None;
        for edit in self.history.undo() {
            self.splice(edit.at, edit.inserted.chars().count(), &edit.removed);
            pos = Some(edit.at + edit.removed.chars().count());
        }
        pos
    }

    /// Re-applies the last undone edit and returns the cursor position after it.
    pub fn redo(&mut self) -> Option<usize> {
        let mut pos = None;
        for edit in self.history.redo() {
            self.splice(edit.at, edit.removed.chars().count(), &edit.inserted);
            pos = Some(edit.at + edit.inserted.chars().count());
        }
        pos
    }

    fn splice(&mut self, at: usize, remove_len: usize, insert: &str) {
        self.text.remove(at..at + remove_len);
        self.text.insert(at, insert);
        self.revision += 1;
        if self.changes.len() == CHANGE_LOG_LEN {
            self.changes.pop_front();
        }
        let tail = self.text.len_chars() - at - insert.chars().count();
        self.changes.push_back((self.revision, at, tail));
    }
}

#[derive(Debug)]
pub enum SaveError {
    NoPath,
    Encode(EncodeError),
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPath => f.write_str("document has no file path"),
            Self::Encode(err) => err.fmt(f),
            Self::Io(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for SaveError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_text(doc: &mut Document, at: usize, text: &str) -> usize {
        text.chars().fold(at, |pos, ch| {
            doc.replace(pos..pos, ch.encode_utf8(&mut [0; 4]))
        })
    }

    #[test]
    fn reports_first_change() {
        let mut doc = Document::new();
        doc.replace(0..0, "hello world");
        let revision = doc.revision();
        assert_eq!(doc.first_change_since(revision), None);
        doc.replace(6..11, "there");
        doc.replace(2..3, "");
        assert_eq!(doc.first_change_since(revision), Some(2));
        doc.undo();
        assert_eq!(doc.first_change_since(revision), Some(2));
        assert_eq!(doc.first_change_since(0), Some(0));
    }

    #[test]
    fn reports_unchanged_start_and_end() {
        let mut doc = Document::new();
        doc.replace(0..0, "hello world");
        let revision = doc.revision();
        doc.replace(6..11, "there!");
        assert_eq!(doc.unchanged_since(revision), Some((6, 0)));
        let revision = doc.revision();
        doc.replace(1..2, "EE");
        doc.replace(4..4, "x");
        assert_eq!(doc.text().to_string(), "hEElxlo there!");
        assert_eq!(doc.unchanged_since(revision), Some((1, 9)));
        assert_eq!(doc.unchanged_since(0), Some((0, 0)));
    }

    #[test]
    fn forgets_old_changes() {
        let mut doc = Document::new();
        for _ in 0..CHANGE_LOG_LEN + 1 {
            doc.replace(0..0, "x");
            doc.seal_undo_group();
        }
        doc.replace(50..50, "y");
        assert_eq!(doc.first_change_since(1), Some(0));
        assert_eq!(doc.first_change_since(doc.revision() - 1), Some(50));
    }

    #[test]
    fn reloads_renames_and_detects_disk_changes() {
        let dir = std::env::temp_dir().join(format!("cuneiedit-disk-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        fs::write(&path, "one\r\ntwo").unwrap();

        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.disk_status(), DiskStatus::Unchanged);
        fs::write(&path, "changed on disk!").unwrap();
        assert_eq!(doc.disk_status(), DiskStatus::Modified);
        doc.reload(None).unwrap();
        assert_eq!(doc.text().to_string(), "changed on disk!");
        assert!(!doc.is_modified());
        assert_eq!(doc.disk_status(), DiskStatus::Unchanged);
        doc.undo();
        assert_eq!(doc.text().to_string(), "one\ntwo", "reload is undoable");

        fs::write(&path, b"Gr\xFC\xDFe").unwrap();
        let latin2 = Encoding::from_label("ISO-8859-2").unwrap();
        doc.reload(Some(latin2)).unwrap();
        assert_eq!(doc.encoding(), latin2);

        let copy = dir.join("copy.txt");
        doc.save_copy(&copy).unwrap();
        assert_eq!(doc.path(), Some(path.as_path()));
        assert!(copy.exists());

        let renamed = dir.join("b.txt");
        assert!(doc.rename(&copy).is_err(), "target exists");
        doc.rename(&renamed).unwrap();
        assert!(!path.exists() && renamed.exists());
        fs::remove_file(&renamed).unwrap();
        assert_eq!(doc.disk_status(), DiskStatus::Deleted);
        doc.acknowledge_disk_change();
        assert!(doc.is_modified());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_format_on_round_trip() {
        let bytes = Encoding::Utf16Le.encode("a\r\nb\r\n").unwrap();
        let doc = Document::from_bytes(bytes.clone());
        assert_eq!(doc.text().to_string(), "a\nb\n");
        assert_eq!(doc.line_ending(), LineEnding::CrLf);
        assert_eq!(doc.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn normalizes_inserted_line_endings() {
        let mut doc = Document::new();
        assert_eq!(doc.replace(0..0, "a\r\nb"), 3);
        assert_eq!(doc.text().to_string(), "a\nb");
    }

    #[test]
    fn groups_typing_by_word() {
        let mut doc = Document::new();
        type_text(&mut doc, 0, "hello world");
        assert_eq!(doc.undo(), Some(6));
        assert_eq!(doc.text().to_string(), "hello ");
        assert_eq!(doc.undo(), Some(0));
        assert_eq!(doc.text().to_string(), "");
        assert_eq!(doc.redo(), Some(6));
        assert_eq!(doc.text().to_string(), "hello ");
    }

    #[test]
    fn groups_backspaces() {
        let mut doc = Document::new();
        doc.replace(0..0, "abcd");
        doc.seal_undo_group();
        doc.replace(3..4, "");
        doc.replace(2..3, "");
        assert_eq!(doc.text().to_string(), "ab");
        assert_eq!(doc.undo(), Some(4));
        assert_eq!(doc.text().to_string(), "abcd");
    }

    #[test]
    fn replaces_many_ranges_as_one_step() {
        let mut doc = Document::new();
        doc.replace(0..0, "ab cd ef");
        let ends = doc.replace_many(&[(6..6, "X".into()), (0..2, "".into()), (3..3, "YY".into())]);
        assert_eq!(doc.text().to_string(), " YYcd Xef");
        assert_eq!(ends, vec![7, 0, 3]);
        doc.replace(0..0, "z");
        doc.undo();
        assert_eq!(doc.undo(), Some(6));
        assert_eq!(doc.text().to_string(), "ab cd ef");
        doc.redo();
        assert_eq!(doc.text().to_string(), " YYcd Xef");
        doc.undo();
        assert_eq!(doc.text().to_string(), "ab cd ef");
    }

    #[test]
    fn sealing_starts_new_group() {
        let mut doc = Document::new();
        type_text(&mut doc, 0, "ab");
        doc.seal_undo_group();
        type_text(&mut doc, 2, "cd");
        doc.undo();
        assert_eq!(doc.text().to_string(), "ab");
    }

    #[test]
    fn tracks_modification_against_saved_state() {
        let dir = std::env::temp_dir().join(format!("cuneiedit-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.txt");

        let mut doc = Document::new();
        assert!(!doc.is_modified());
        type_text(&mut doc, 0, "ab");
        assert!(doc.is_modified());
        doc.save_as(&path).unwrap();
        assert!(!doc.is_modified());

        type_text(&mut doc, 2, "c");
        assert!(doc.is_modified());
        doc.undo();
        assert!(
            !doc.is_modified(),
            "typing after save must not merge into saved edit"
        );

        doc.set_encoding(Encoding::Utf8Bom);
        assert!(doc.is_modified());
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"\xEF\xBB\xBFab");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_reports_unencodable_position() {
        let mut doc = Document::new();
        doc.replace(0..0, "a\nb✓");
        doc.set_line_ending(LineEnding::CrLf);
        doc.set_encoding(Encoding::Windows1252);
        assert_eq!(doc.to_bytes().unwrap_err().char_index, 3);
    }
}
