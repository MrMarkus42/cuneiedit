//! The only places where Pilcrow touches the system outside its own files.
//!
//! - [`reveal`] starts the platform file manager with a fixed program path and the file as a
//!   single argument (no shell, so file names cannot inject commands).
//! - [`move_to_trash`] renames the file into the user's trash folder.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs, io};

/// Whether this platform can move files to the trash.
pub const HAS_TRASH: bool = cfg!(any(target_os = "macos", target_os = "linux"));

/// Shows the file selected in Finder (macOS), Explorer (Windows) or its folder (Linux).
pub fn reveal(path: &Path) -> io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("/usr/bin/open");
        command.arg("-R").arg(path);
        command
    } else if cfg!(windows) {
        let windir = env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let mut command = Command::new(PathBuf::from(windir).join("explorer.exe"));
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        command.arg(arg);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

/// Moves the file into the trash and returns where it went. Fails if the trash is on
/// another volume.
pub fn move_to_trash(path: &Path) -> io::Result<PathBuf> {
    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    let home = env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or(io::ErrorKind::NotFound)?;
    if cfg!(target_os = "macos") {
        let target = unique_target(&home.join(".Trash"), name)?;
        fs::rename(path, &target)?;
        Ok(target)
    } else if cfg!(target_os = "linux") {
        // freedesktop.org trash specification: the file goes to `files/`, a
        // `.trashinfo` file with the original path to `info/`.
        let trash = env::var_os("XDG_DATA_HOME")
            .filter(|d| !d.is_empty())
            .map_or_else(|| home.join(".local/share"), PathBuf::from)
            .join("Trash");
        fs::create_dir_all(trash.join("files"))?;
        fs::create_dir_all(trash.join("info"))?;
        let target = unique_target(&trash.join("files"), name)?;
        let target_name = target.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        let mut info_name = target_name.to_os_string();
        info_name.push(".trashinfo");
        let date = jiff::Zoned::now().strftime("%Y-%m-%dT%H:%M:%S").to_string();
        let info = format!(
            "[Trash Info]\nPath={}\nDeletionDate={date}\n",
            percent_encode(&path.to_string_lossy())
        );
        let info_path = trash.join("info").join(info_name);
        fs::write(&info_path, info)?;
        fs::rename(path, &target).inspect_err(|_| {
            let _ = fs::remove_file(&info_path);
        })?;
        Ok(target)
    } else {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// `dir/name`, or `dir/name 2`, `dir/name 3`, … if it exists.
fn unique_target(dir: &Path, name: &std::ffi::OsStr) -> io::Result<PathBuf> {
    let first = dir.join(name);
    if !first.exists() {
        return Ok(first);
    }
    let original = Path::new(name);
    let stem = original.file_stem().unwrap_or(name).to_string_lossy();
    let extension = original
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..10_000)
        .map(|n| dir.join(format!("{stem} {n}{extension}")))
        .find(|candidate| !candidate.exists())
        .ok_or_else(|| io::ErrorKind::AlreadyExists.into())
}

/// Percent-encodes everything except unreserved characters and `/`.
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encodes_paths() {
        assert_eq!(percent_encode("/a b/ä.txt"), "/a%20b/%C3%A4.txt");
    }

    #[test]
    fn finds_a_free_name() {
        let dir = env::temp_dir().join(format!("pilcrow-trash-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), "").unwrap();
        fs::write(dir.join("a 2.txt"), "").unwrap();
        let target = unique_target(&dir, "a.txt".as_ref()).unwrap();
        assert_eq!(target, dir.join("a 3.txt"));
        assert_eq!(unique_target(&dir, "b".as_ref()).unwrap(), dir.join("b"));
        fs::remove_dir_all(dir).unwrap();
    }
}
