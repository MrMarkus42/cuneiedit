//! One instance for all files: a second start hands its files to the running instance and
//! exits. The channel is local and only usable by the user: a Unix domain socket in a private
//! folder next to the settings, or a named pipe on Windows (see `cuneiedit-windows`).
//!
//! A message is a header followed by absolute paths, each with its length in front. Without
//! paths it only brings the window to the front.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use eframe::egui;

/// Larger messages are ignored.
const MAX_MESSAGE: usize = 1 << 20;
const MAX_FILES: usize = 1000;
const HEADER: &[u8] = b"cuneiedit-open-1\n";

/// Starts a separate instance that neither hands over its files nor receives any.
pub const NEW_INSTANCE: &str = "--new-instance";

/// How this process starts.
pub enum Start {
    /// The files went to the running instance; this one can exit.
    Forwarded,
    /// This is the main instance and receives files from later starts.
    Main(Listener),
    /// The channel cannot be used; run on its own.
    Alone,
}

/// Hands `files` to the running instance or becomes the main instance.
pub fn start(files: &[PathBuf]) -> Start {
    let Some(message) = encode(files) else {
        return Start::Alone;
    };
    // Two starts at the same time: the one that does not get the channel sends to the other.
    for _ in 0..3 {
        match platform::send(&message) {
            Sent::Delivered => return Start::Forwarded,
            Sent::Failed => return Start::Alone,
            Sent::NoInstance => {}
        }
        if let Some(listener) = platform::listen() {
            return Start::Main(Listener(listener));
        }
    }
    Start::Alone
}

pub struct Listener(platform::Listener);

impl Listener {
    /// Receives files in the background; each message wakes up the window.
    pub fn spawn(self, ctx: egui::Context) -> Receiver<Vec<PathBuf>> {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            platform::serve(self.0, |data| {
                let Some(files) = decode(&data) else {
                    return true;
                };
                let open = sender.send(files).is_ok();
                ctx.request_repaint();
                open
            });
        });
        receiver
    }
}

enum Sent {
    Delivered,
    NoInstance,
    Failed,
}

fn encode(files: &[PathBuf]) -> Option<Vec<u8>> {
    let mut out = HEADER.to_vec();
    for file in files.iter().take(MAX_FILES) {
        // The running instance has its own working folder.
        let path = std::path::absolute(file).ok()?;
        let bytes = platform::path_bytes(&path);
        out.extend_from_slice(&u32::try_from(bytes.len()).ok()?.to_le_bytes());
        out.extend_from_slice(&bytes);
    }
    (out.len() <= MAX_MESSAGE).then_some(out)
}

fn decode(data: &[u8]) -> Option<Vec<PathBuf>> {
    if data.len() > MAX_MESSAGE {
        return None;
    }
    let mut rest = data.strip_prefix(HEADER)?;
    let mut files = Vec::new();
    while !rest.is_empty() {
        let (len, tail) = rest.split_first_chunk::<4>()?;
        let len = usize::try_from(u32::from_le_bytes(*len)).ok()?;
        if len > tail.len() || files.len() >= MAX_FILES {
            return None;
        }
        let (bytes, tail) = tail.split_at(len);
        let path = platform::bytes_path(bytes)?;
        if !path.is_absolute() {
            return None;
        }
        files.push(path);
        rest = tail;
    }
    Some(files)
}

#[cfg(unix)]
mod platform {
    use std::ffi::OsStr;
    use std::fs::{self, Permissions};
    use std::io::{self, Read as _, Write as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::Duration;

    use super::{MAX_MESSAGE, Sent};
    use crate::session::create_private_dir;
    use crate::settings::config_file;

    const TIMEOUT: Duration = Duration::from_secs(2);

    pub type Listener = UnixListener;

    /// The socket, in a folder only the user can enter.
    fn socket() -> Option<PathBuf> {
        Some(config_file("instance")?.join("socket"))
    }

    pub fn send(message: &[u8]) -> Sent {
        let Some(path) = socket() else {
            return Sent::Failed;
        };
        match UnixStream::connect(path) {
            Ok(mut stream) => {
                let sent = stream.set_write_timeout(Some(TIMEOUT)).is_ok()
                    && stream.write_all(message).is_ok();
                if sent { Sent::Delivered } else { Sent::Failed }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                Sent::NoInstance
            }
            Err(_) => Sent::Failed,
        }
    }

    pub fn listen() -> Option<Listener> {
        let path = socket()?;
        let dir = path.parent()?;
        create_private_dir(dir).ok()?;
        // Only succeeds for the owner; also corrects a folder created otherwise.
        fs::set_permissions(dir, Permissions::from_mode(0o700)).ok()?;
        match UnixListener::bind(&path) {
            Ok(listener) => Some(listener),
            Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
                // Left behind by an instance that has ended, unless one answers now.
                if UnixStream::connect(&path).is_ok() {
                    return None;
                }
                fs::remove_file(&path).ok()?;
                UnixListener::bind(&path).ok()
            }
            Err(_) => None,
        }
    }

    /// Calls `receive` with each message until it returns `false`.
    pub fn serve(listener: Listener, mut receive: impl FnMut(Vec<u8>) -> bool) {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                thread::sleep(Duration::from_millis(100));
                continue;
            };
            if stream.set_read_timeout(Some(TIMEOUT)).is_err() {
                continue;
            }
            let mut data = Vec::new();
            let limit = MAX_MESSAGE as u64 + 1;
            if stream.take(limit).read_to_end(&mut data).is_ok() && !receive(data) {
                return;
            }
        }
    }

    pub fn path_bytes(path: &Path) -> Vec<u8> {
        path.as_os_str().as_bytes().to_vec()
    }

    pub fn bytes_path(bytes: &[u8]) -> Option<PathBuf> {
        (!bytes.contains(&0)).then(|| PathBuf::from(OsStr::from_bytes(bytes)))
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    use std::path::{Path, PathBuf};

    use cuneiedit_windows::instance::{self, Server};

    use super::{MAX_MESSAGE, Sent};
    use crate::settings::config_file;

    pub type Listener = Server;

    /// The pipe name; test runs with another settings folder get their own.
    fn name() -> String {
        let folder = config_file("").unwrap_or_default();
        // FNV-1a, to keep the name short.
        let hash = folder
            .as_os_str()
            .encode_wide()
            .fold(0xcbf2_9ce4_8422_2325u64, |hash, unit| {
                (hash ^ u64::from(unit)).wrapping_mul(0x0100_0000_01b3)
            });
        format!("CuneiEdit-{hash:016x}")
    }

    pub fn send(message: &[u8]) -> Sent {
        match instance::send(&name(), message) {
            instance::Sent::Delivered => Sent::Delivered,
            instance::Sent::NoInstance => Sent::NoInstance,
            instance::Sent::Failed => Sent::Failed,
        }
    }

    pub fn listen() -> Option<Listener> {
        Server::listen(&name())
    }

    /// Calls `receive` with each message until it returns `false`.
    pub fn serve(server: Listener, mut receive: impl FnMut(Vec<u8>) -> bool) {
        while let Some(data) = server.receive(MAX_MESSAGE + 1) {
            if !receive(data) {
                return;
            }
        }
    }

    pub fn path_bytes(path: &Path) -> Vec<u8> {
        path.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    pub fn bytes_path(bytes: &[u8]) -> Option<PathBuf> {
        let (pairs, rest) = bytes.as_chunks::<2>();
        if !rest.is_empty() {
            return None;
        }
        let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        (!units.contains(&0)).then(|| PathBuf::from(OsString::from_wide(&units)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_paths() {
        let dir = std::env::temp_dir();
        let files = vec![dir.join("b c.txt"), dir.join("ä").join("€.rs")];
        let message = encode(&files).unwrap();
        assert_eq!(decode(&message).unwrap(), files);
        assert_eq!(decode(HEADER).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn rejects_bad_messages() {
        let message = encode(&[std::env::temp_dir()]).unwrap();
        assert!(decode(&message[..message.len() - 1]).is_none(), "cut off");
        assert!(decode(&message[1..]).is_none(), "no header");
        assert!(decode(b"").is_none());
        let mut huge = HEADER.to_vec();
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&huge).is_none());
        let mut relative = HEADER.to_vec();
        relative.extend_from_slice(&1u32.to_le_bytes());
        relative.push(b'a');
        assert!(decode(&relative).is_none());
        assert!(decode(&vec![0; MAX_MESSAGE + 1]).is_none());
    }
}
