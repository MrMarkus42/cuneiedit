//! A named pipe through which a second start hands its files to the running instance.
//!
//! Only the current user can open the pipe (its access list names the user's SID), remote
//! clients are rejected, and a client checks that the pipe belongs to a process of the same
//! user before it sends anything.

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::os::windows::io::AsRawHandle as _;
use std::ptr;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND, ReadFile,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

use crate::wide_nul;

/// How often a client tries again while the server handles another client.
const BUSY_RETRIES: usize = 20;
const BUSY_WAIT: Duration = Duration::from_millis(100);
/// Largest `TOKEN_USER` accepted (a SID has at most 68 bytes).
const MAX_TOKEN_USER: u32 = 1024;

/// A handle that is closed when dropped.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned by this value.
        unsafe { CloseHandle(self.0) };
    }
}

/// The user a process runs as: its `TOKEN_USER`, in a buffer aligned for it.
struct User(Vec<u64>);

impl User {
    fn of(process: HANDLE) -> Option<Self> {
        let mut token = ptr::null_mut();
        // SAFETY: `process` is a valid process handle; the token handle is closed by `Handle`.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return None;
        }
        let token = Handle(token);
        let mut len = 0;
        // SAFETY: an empty buffer only asks for the size needed.
        unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut len) };
        if len == 0 || len > MAX_TOKEN_USER {
            return None;
        }
        let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: the buffer holds `len` bytes and is aligned for `TOKEN_USER`.
        let ok = unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                len,
                &mut len,
            )
        };
        (ok != 0).then_some(Self(buffer))
    }

    fn current() -> Option<Self> {
        // SAFETY: returns a pseudo handle that needs no closing.
        Self::of(unsafe { GetCurrentProcess() })
    }

    fn sid(&self) -> *mut c_void {
        // SAFETY: `GetTokenInformation` filled the buffer with a `TOKEN_USER`, whose SID
        // points into the same buffer.
        unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    fn sid_string(&self) -> Option<String> {
        let mut text = ptr::null_mut();
        // SAFETY: the SID is valid; the string is freed with `LocalFree` below.
        if unsafe { ConvertSidToStringSidW(self.sid(), &mut text) } == 0 {
            return None;
        }
        let mut len = 0;
        // SAFETY: the string is NUL-terminated.
        while unsafe { *text.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: `len` characters before the NUL were just read.
        let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) }).ok();
        // SAFETY: allocated by `ConvertSidToStringSidW`.
        unsafe { LocalFree(text.cast()) };
        sid
    }

    fn same_as(&self, other: &Self) -> bool {
        // SAFETY: both SIDs are valid.
        unsafe { EqualSid(self.sid(), other.sid()) != 0 }
    }
}

fn pipe_path(name: &str, sid: &str) -> String {
    format!(r"\\.\pipe\{name}-{sid}")
}

/// The receiving end; only one can exist per name and user.
pub struct Server(Handle);

// SAFETY: a pipe handle may be used from any thread.
unsafe impl Send for Server {}

impl Server {
    /// Creates the pipe, or returns `None` if it exists already (another instance is
    /// running) or cannot be created.
    pub fn listen(name: &str) -> Option<Self> {
        let sid = User::current()?.sid_string()?;
        let path = wide_nul(&pipe_path(name, &sid));
        // Protected access list: the user only, full access.
        let sddl = wide_nul(&format!("D:P(A;;GA;;;{sid})"));
        let mut descriptor = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; the descriptor is freed with `LocalFree` below.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return None;
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        // SAFETY: `path` is NUL-terminated and `attributes` is valid during the call (the
        // pipe keeps its own copy of the descriptor).
        let pipe = unsafe {
            CreateNamedPipeW(
                path.as_ptr(),
                PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                64 * 1024,
                0,
                &attributes,
            )
        };
        // SAFETY: allocated by `ConvertStringSecurityDescriptorToSecurityDescriptorW`.
        unsafe { LocalFree(descriptor) };
        (pipe != INVALID_HANDLE_VALUE).then(|| Self(Handle(pipe)))
    }

    /// Waits for a client and returns what it sent, at most `limit` bytes (more is cut off).
    /// Returns `None` if the pipe no longer works.
    pub fn receive(&self, limit: usize) -> Option<Vec<u8>> {
        // SAFETY: the pipe handle is open; no overlapped I/O.
        let connected = unsafe { ConnectNamedPipe(self.0.0, ptr::null_mut()) } != 0;
        let error = if connected {
            0
        } else {
            // SAFETY: no arguments.
            unsafe { GetLastError() }
        };
        if error == ERROR_NO_DATA {
            // The client has come and gone already.
            // SAFETY: the pipe handle is open.
            unsafe { DisconnectNamedPipe(self.0.0) };
            return Some(Vec::new());
        }
        if !connected && error != ERROR_PIPE_CONNECTED {
            return None;
        }
        let mut data = Vec::new();
        let mut buffer = [0u8; 4096];
        while data.len() < limit {
            let mut read = 0;
            // SAFETY: `buffer` holds `buffer.len()` bytes; no overlapped I/O.
            let ok = unsafe {
                ReadFile(
                    self.0.0,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    &mut read,
                    ptr::null_mut(),
                )
            };
            // Fails with "broken pipe" once the client has closed its end.
            if ok == 0 || read == 0 {
                break;
            }
            data.extend_from_slice(&buffer[..read as usize]);
        }
        data.truncate(limit);
        // SAFETY: the pipe handle is open.
        unsafe { DisconnectNamedPipe(self.0.0) };
        Some(data)
    }
}

/// What became of a message for the running instance.
pub enum Sent {
    Delivered,
    /// No instance is listening.
    NoInstance,
    Failed,
}

/// Sends `data` to the instance listening on `name`, after checking that it runs as the same
/// user, and lets it bring its window to the front.
pub fn send(name: &str, data: &[u8]) -> Sent {
    let Some(user) = User::current() else {
        return Sent::Failed;
    };
    let Some(sid) = user.sid_string() else {
        return Sent::Failed;
    };
    let path = pipe_path(name, &sid);
    for _ in 0..BUSY_RETRIES {
        match OpenOptions::new().write(true).open(&path) {
            Ok(mut pipe) => {
                let mut pid = 0;
                // SAFETY: the handle is an open pipe.
                let ok = unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) };
                if ok == 0 || !runs_as(pid, &user) {
                    return Sent::Failed;
                }
                // SAFETY: no pointers; fails harmlessly if this process may not allow it.
                unsafe { AllowSetForegroundWindow(pid) };
                return match pipe.write_all(data) {
                    Ok(()) => Sent::Delivered,
                    Err(_) => Sent::Failed,
                };
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Sent::NoInstance,
            Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                thread::sleep(BUSY_WAIT);
            }
            Err(_) => return Sent::Failed,
        }
    }
    Sent::Failed
}

fn runs_as(pid: u32, user: &User) -> bool {
    // SAFETY: no pointers; the handle is closed by `Handle`.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return false;
    }
    let process = Handle(process);
    User::of(process.0).is_some_and(|owner| owner.same_as(user))
}
