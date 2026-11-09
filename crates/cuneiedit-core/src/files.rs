//! Find and replace in files below a folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::code::{LineHits, hits_by_line};
use crate::{Document, SaveError, Search};

/// Larger files are skipped.
pub const MAX_FILE_SIZE: u64 = 64 * 1024 * 1024;

/// File name patterns like `*.rs *.txt !*.min.js`; `!` excludes. Empty matches everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileFilter {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl FileFilter {
    pub fn parse(patterns: &str) -> Self {
        let mut filter = Self::default();
        for pattern in patterns.split([' ', ';', ',']).filter(|p| !p.is_empty()) {
            match pattern.strip_prefix('!') {
                Some(excluded) if !excluded.is_empty() => {
                    filter.exclude.push(excluded.to_lowercase());
                }
                Some(_) => {}
                None => filter.include.push(pattern.to_lowercase()),
            }
        }
        filter
    }

    /// Whether a file name matches, ignoring case.
    pub fn matches(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        let hit = |pattern: &String| wildcard(pattern, &name);
        (self.include.is_empty() || self.include.iter().any(hit)) && !self.exclude.iter().any(hit)
    }
}

/// Matches `*` (any run of chars) and `?` (one char).
fn wildcard(pattern: &str, name: &str) -> bool {
    let (pattern, name): (Vec<char>, Vec<char>) =
        (pattern.chars().collect(), name.chars().collect());
    let (mut p, mut n) = (0, 0);
    let mut backtrack = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                backtrack = Some((p, n));
                p += 1;
            }
            Some(&c) if c == '?' || c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match backtrack {
                Some((star, matched)) => {
                    p = star + 1;
                    n = matched + 1;
                    backtrack = Some((star, matched + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WalkOptions {
    pub subfolders: bool,
    /// Include files and folders whose name starts with a dot.
    pub hidden: bool,
}

/// Calls `visit` for every matching file below `dir`, in name order, until it returns
/// `false`. Symbolic links to folders are not followed, so loops are impossible.
/// Unreadable folders are skipped.
pub fn walk(
    dir: &Path,
    filter: &FileFilter,
    options: WalkOptions,
    visit: &mut dyn FnMut(&Path) -> bool,
) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return true;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !options.hidden && name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let keep_going = if kind.is_dir() {
            !options.subfolders || walk(&path, filter, options, visit)
        } else if filter.matches(&name) && (kind.is_file() || path.is_file()) {
            visit(&path)
        } else {
            true
        };
        if !keep_going {
            return false;
        }
    }
    true
}

/// Whether `bytes` look like a binary file: a NUL byte near the start, unless it is UTF-16.
pub fn is_binary(bytes: &[u8]) -> bool {
    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    !utf16 && bytes[..bytes.len().min(8000)].contains(&0)
}

/// One entry of a folder listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Entries of `dir`, folders first, each sorted by name ignoring case. Symbolic links to
/// folders are left out, so a tree built from this cannot loop.
pub fn list_dir(dir: &Path, hidden: bool) -> io::Result<Vec<DirEntry>> {
    let mut entries: Vec<DirEntry> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                return None;
            }
            let kind = entry.file_type().ok()?;
            let path = entry.path();
            if kind.is_symlink() && path.is_dir() {
                return None;
            }
            Some(DirEntry {
                name,
                path,
                is_dir: kind.is_dir(),
            })
        })
        .collect();
    entries.sort_by_cached_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
    Ok(entries)
}

/// Names in the folder of a path being typed (`typed`, relative to `base`) that continue its
/// last part. Folders end with `/`. `~/` stands for the home folder.
pub fn complete_path(base: &Path, typed: &str, limit: usize) -> Vec<String> {
    let (dir_part, prefix) = typed.rsplit_once('/').unwrap_or(("", typed));
    let dir = if typed.starts_with('/') {
        PathBuf::from(if dir_part.is_empty() { "/" } else { dir_part })
    } else if let Some(rest) = dir_part.strip_prefix('~') {
        match std::env::var_os("HOME").filter(|h| !h.is_empty()) {
            Some(home) => PathBuf::from(home).join(rest.trim_start_matches('/')),
            None => return Vec::new(),
        }
    } else {
        base.join(dir_part)
    };
    let Ok(entries) = list_dir(&dir, prefix.starts_with('.')) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter(|entry| entry.name.starts_with(prefix))
        .map(|entry| {
            if entry.is_dir {
                entry.name + "/"
            } else {
                entry.name
            }
        })
        .filter(|name| name != prefix)
        .take(limit)
        .collect()
}

/// Matches in one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHits {
    pub path: PathBuf,
    pub lines: Vec<LineHits>,
}

/// Reads a text file for searching; `None` for binary or too large files.
fn read_text(path: &Path) -> io::Result<Option<Document>> {
    if fs::metadata(path)?.len() > MAX_FILE_SIZE {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    if is_binary(&bytes) {
        return Ok(None);
    }
    let mut doc = Document::from_bytes(bytes);
    doc.set_path(Some(path.to_path_buf()));
    Ok(Some(doc))
}

/// Matches of `search` in the file, or `None` if there are none or it is not a text file.
pub fn find_in_file(path: &Path, search: &Search) -> io::Result<Option<FileHits>> {
    let Some(doc) = read_text(path)? else {
        return Ok(None);
    };
    let matches = search.find_all(&doc);
    Ok((!matches.is_empty()).then(|| FileHits {
        path: path.to_path_buf(),
        lines: hits_by_line(&doc, &matches),
    }))
}

/// Replaces all matches in the file and saves it in its original encoding and line ending.
/// Returns the number of replacements; files without matches are not written.
pub fn replace_in_file(
    path: &Path,
    search: &Search,
    replacement: &str,
) -> Result<usize, SaveError> {
    let Some(mut doc) = read_text(path).map_err(SaveError::Io)? else {
        return Ok(0);
    };
    let count = search.replace_all(&mut doc, replacement, None);
    if count > 0 {
        doc.save()?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchQuery;

    #[test]
    fn matches_wildcards() {
        assert!(wildcard("*.rs", "main.rs"));
        assert!(wildcard("a?c", "abc"));
        assert!(wildcard("*a*b*", "xxaYYbzz"));
        assert!(!wildcard("*.rs", "main.rsx"));
        assert!(!wildcard("a?c", "ac"));
        assert!(wildcard("*", ""));
    }

    #[test]
    fn filters_file_names() {
        let filter = FileFilter::parse("*.rs; *.TXT !test*");
        assert!(filter.matches("Main.RS"));
        assert!(filter.matches("notes.txt"));
        assert!(!filter.matches("test.rs"));
        assert!(!filter.matches("image.png"));
        assert!(FileFilter::parse("  ").matches("anything"));
        assert!(!FileFilter::parse("!*.bak").matches("x.bak"));
    }

    #[test]
    fn detects_binary_files() {
        assert!(is_binary(b"PK\x03\x04\0\0"));
        assert!(!is_binary(b"plain text"));
        assert!(!is_binary(&[0xFF, 0xFE, b'a', 0]));
    }

    #[test]
    fn lists_folders_and_completes_paths() {
        let dir = std::env::temp_dir().join(format!("cuneiedit-list-{}", std::process::id()));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("b.txt"), "").unwrap();
        fs::write(dir.join("A.md"), "").unwrap();
        fs::write(dir.join(".hidden"), "").unwrap();
        fs::write(dir.join("src/main.rs"), "").unwrap();
        let names: Vec<String> = list_dir(&dir, false)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["src", "A.md", "b.txt"]);
        assert_eq!(list_dir(&dir, true).unwrap().len(), 4);

        assert_eq!(complete_path(&dir, "./s", 10), vec!["src/"]);
        assert_eq!(complete_path(&dir, "src/m", 10), vec!["main.rs"]);
        assert_eq!(complete_path(&dir, "./.h", 10), vec![".hidden"]);
        let absolute = format!("{}/b", dir.display());
        assert_eq!(complete_path(Path::new("/"), &absolute, 10), vec!["b.txt"]);
        assert!(complete_path(&dir, "nope/x", 10).is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("cuneiedit-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("sub")).unwrap();
            fs::create_dir_all(dir.join(".hidden")).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn walks_finds_and_replaces() {
        let temp = TempDir::new("files");
        let dir = &temp.0;
        fs::write(dir.join("a.txt"), "one fish\ntwo fish\r\n").unwrap();
        fs::write(dir.join("b.bin"), b"fish\0").unwrap();
        fs::write(dir.join("sub/c.txt"), "red fish").unwrap();
        fs::write(dir.join(".hidden/d.txt"), "fish").unwrap();

        let collect = |options| {
            let mut found = Vec::new();
            walk(dir, &FileFilter::parse("*.txt"), options, &mut |path| {
                found.push(path.strip_prefix(dir).unwrap().to_path_buf());
                true
            });
            found
        };
        let flat = WalkOptions::default();
        assert_eq!(collect(flat), vec![PathBuf::from("a.txt")]);
        let deep = WalkOptions {
            subfolders: true,
            hidden: false,
        };
        assert_eq!(collect(deep).len(), 2);
        let all = WalkOptions {
            subfolders: true,
            hidden: true,
        };
        assert_eq!(collect(all).len(), 3);

        let query = SearchQuery {
            pattern: "fish".into(),
            ..SearchQuery::default()
        };
        let search = Search::new(&query).unwrap().unwrap();
        let hits = find_in_file(&dir.join("a.txt"), &search).unwrap().unwrap();
        assert_eq!(hits.lines.len(), 2);
        assert_eq!(hits.lines[1].hits, vec![4..8]);
        assert_eq!(find_in_file(&dir.join("b.bin"), &search).unwrap(), None);

        assert_eq!(
            replace_in_file(&dir.join("a.txt"), &search, "cat").unwrap(),
            2
        );
        let text = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert_eq!(text.replace('\r', ""), "one cat\ntwo cat\n");
        assert_eq!(
            replace_in_file(&dir.join("b.bin"), &search, "cat").unwrap(),
            0
        );
    }
}
