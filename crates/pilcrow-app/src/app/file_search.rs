//! Find and replace in files on a background thread, so the window stays responsive.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use pilcrow_core::Search;
use pilcrow_core::files::{FileFilter, FileHits, WalkOptions, find_in_file, replace_in_file, walk};

/// What to search, as entered in the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilesForm {
    pub dir: String,
    pub filters: String,
    pub subfolders: bool,
    pub hidden: bool,
}

impl Default for FilesForm {
    fn default() -> Self {
        Self {
            dir: String::new(),
            filters: "*.*".into(),
            subfolders: true,
            hidden: false,
        }
    }
}

enum Message {
    Scanned(usize),
    Found(FileHits),
    Replaced(usize),
    Done,
}

pub struct FileSearch {
    receiver: Receiver<Message>,
    cancel: Arc<AtomicBool>,
    pub replacing: bool,
    pub scanned: usize,
    pub replaced: usize,
    pub replaced_files: usize,
    pub done: bool,
}

impl FileSearch {
    /// Starts searching; with a `replacement`, matching files are rewritten.
    pub fn start(form: &FilesForm, search: Search, replacement: Option<String>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let replacing = replacement.is_some();
        let dir = PathBuf::from(form.dir.trim());
        let filter = FileFilter::parse(&form.filters);
        let options = WalkOptions {
            subfolders: form.subfolders,
            hidden: form.hidden,
        };
        let stop = Arc::clone(&cancel);
        thread::spawn(move || {
            let mut scanned = 0;
            walk(&dir, &filter, options, &mut |path| {
                if stop.load(Ordering::Relaxed) {
                    return false;
                }
                scanned += 1;
                if scanned % 50 == 0 {
                    let _ = sender.send(Message::Scanned(scanned));
                }
                // Unreadable files are skipped.
                let message = match &replacement {
                    None => find_in_file(path, &search)
                        .ok()
                        .flatten()
                        .map(Message::Found),
                    Some(replacement) => replace_in_file(path, &search, replacement)
                        .ok()
                        .filter(|&count| count > 0)
                        .map(Message::Replaced),
                };
                // The receiver is gone if the search was discarded.
                message.is_none_or(|message| sender.send(message).is_ok())
            });
            let _ = sender.send(Message::Scanned(scanned));
            let _ = sender.send(Message::Done);
        });
        Self {
            receiver,
            cancel,
            replacing,
            scanned: 0,
            replaced: 0,
            replaced_files: 0,
            done: false,
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Takes the files found since the last call.
    pub fn poll(&mut self) -> Vec<FileHits> {
        let mut found = Vec::new();
        loop {
            match self.receiver.try_recv() {
                Ok(Message::Scanned(count)) => self.scanned = count,
                Ok(Message::Found(hits)) => found.push(hits),
                Ok(Message::Replaced(count)) => {
                    self.replaced += count;
                    self.replaced_files += 1;
                }
                Ok(Message::Done) | Err(TryRecvError::Disconnected) => {
                    self.done = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        found
    }
}

impl Drop for FileSearch {
    fn drop(&mut self) {
        self.cancel();
    }
}
