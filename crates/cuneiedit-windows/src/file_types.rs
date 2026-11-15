//! File type registration for the current user, under `HKEY_CURRENT_USER` only.
//!
//! CuneiEdit registers one ProgID, lists itself under "Open with" for each chosen extension,
//! and declares its capabilities so Windows' default apps settings show it. Where the user
//! has not picked a default app in Windows (the protected `UserChoice`), the extension's
//! per-user default is set to CuneiEdit; an earlier per-user default is kept and restored
//! when the extension is deselected.

use std::ffi::c_void;
use std::io;
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_NONE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RRF_RT_ANY, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW,
    RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows_sys::Win32::UI::Shell::{
    ASSOCF_NONE, ASSOCSTR_EXECUTABLE, AssocQueryStringW, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
    SHChangeNotify, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::wide_nul;

const PROG_ID: &str = "CuneiEdit.Document";
const APP_NAME: &str = "CuneiEdit";
const CLASSES: &str = r"Software\Classes";
const CAPABILITIES: &str = r"Software\CuneiEdit\Capabilities";
const REGISTERED_APPS: &str = r"Software\RegisteredApplications";
/// Holds the per-user default an extension had before CuneiEdit set itself.
const PREVIOUS: &str = "CuneiEdit.Previous";
/// Longest string value read (in UTF-16 units).
const MAX_VALUE: usize = 1024;

fn check(code: WIN32_ERROR) -> io::Result<()> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}

/// An open registry key under `HKEY_CURRENT_USER`, closed when dropped.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key is open and owned by this value.
        unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    /// Opens `path`, creating it if needed.
    fn create(path: &str) -> io::Result<Self> {
        let path = wide_nul(path);
        let mut key = ptr::null_mut();
        // SAFETY: `path` is NUL-terminated; the new handle is owned by the returned value.
        check(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_WRITE,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        })?;
        Ok(Self(key))
    }

    /// Opens `path` for reading, if it exists.
    fn open(path: &str) -> Option<Self> {
        let path = wide_nul(path);
        let mut key = ptr::null_mut();
        // SAFETY: `path` is NUL-terminated; the new handle is owned by the returned value.
        let code =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_READ, &mut key) };
        (code == ERROR_SUCCESS).then_some(Self(key))
    }

    /// Sets the string value `name` (`""` is the key's default value).
    fn set(&self, name: &str, value: &str) -> io::Result<()> {
        let name = wide_nul(name);
        let value = wide_nul(value);
        // SAFETY: both strings are NUL-terminated; the size covers `value` with its NUL.
        check(unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        })
    }

    /// Sets the empty, typeless value `name` (as `OpenWithProgids` expects).
    fn set_empty(&self, name: &str) -> io::Result<()> {
        let name = wide_nul(name);
        // SAFETY: `name` is NUL-terminated; no data is passed.
        check(unsafe { RegSetValueExW(self.0, name.as_ptr(), 0, REG_NONE, ptr::null(), 0) })
    }

    /// The string value `name`, if it exists.
    fn get(&self, name: &str) -> Option<String> {
        let name = wide_nul(name);
        let mut buffer = vec![0u16; MAX_VALUE];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: `name` is NUL-terminated; `buffer` holds `size` bytes.
        let code = unsafe {
            RegGetValueW(
                self.0,
                ptr::null(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS {
            return None;
        }
        let len = (size as usize / 2).min(buffer.len());
        let text = String::from_utf16_lossy(&buffer[..len]);
        Some(text.trim_end_matches('\0').to_owned())
    }

    /// Whether the value `name` exists (of any type).
    fn has(&self, name: &str) -> bool {
        let name = wide_nul(name);
        let mut size = 0;
        // SAFETY: `name` is NUL-terminated; no buffer is passed, only the size is asked for.
        let code = unsafe {
            RegGetValueW(
                self.0,
                ptr::null(),
                name.as_ptr(),
                RRF_RT_ANY,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut size,
            )
        };
        code == ERROR_SUCCESS
    }
}

/// Removes the value `name` of the key `path`; a missing key or value is fine.
fn delete_value(path: &str, name: &str) -> io::Result<()> {
    let path = wide_nul(path);
    let name = wide_nul(name);
    // SAFETY: both strings are NUL-terminated.
    let code = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, path.as_ptr(), name.as_ptr()) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check(code)
}

/// Removes the key `path` with everything below it; a missing key is fine.
fn delete_tree(path: &str) -> io::Result<()> {
    let path = wide_nul(path);
    // SAFETY: `path` is NUL-terminated.
    let code = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check(code)
}

/// The command line Windows runs to open a file.
fn open_command(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

fn extension_key(extension: &str) -> String {
    format!(r"{CLASSES}\.{extension}")
}

/// Registers CuneiEdit (at `exe`) for exactly the `selected` extensions out of `all` (both
/// without the dot); the others in `all` are unregistered. `description` names the file
/// type in Explorer.
pub fn register(exe: &Path, all: &[&str], selected: &[&str], description: &str) -> io::Result<()> {
    let command = open_command(exe);
    let icon = format!("\"{}\",0", exe.display());
    let prog_id = Key::create(&format!(r"{CLASSES}\{PROG_ID}"))?;
    prog_id.set("", description)?;
    Key::create(&format!(r"{CLASSES}\{PROG_ID}\DefaultIcon"))?.set("", &icon)?;
    Key::create(&format!(r"{CLASSES}\{PROG_ID}\shell\open\command"))?.set("", &command)?;

    let exe_name = exe
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "CuneiEdit.exe".to_owned());
    let application = format!(r"{CLASSES}\Applications\{exe_name}");
    Key::create(&application)?.set("FriendlyAppName", APP_NAME)?;
    Key::create(&format!(r"{application}\shell\open\command"))?.set("", &command)?;
    let supported = Key::create(&format!(r"{application}\SupportedTypes"))?;

    let capabilities = Key::create(CAPABILITIES)?;
    capabilities.set("ApplicationName", APP_NAME)?;
    capabilities.set("ApplicationDescription", description)?;
    let associations = Key::create(&format!(r"{CAPABILITIES}\FileAssociations"))?;
    Key::create(REGISTERED_APPS)?.set(APP_NAME, CAPABILITIES)?;

    for &extension in all {
        let dotted = format!(".{extension}");
        if selected.contains(&extension) {
            supported.set(&dotted, "")?;
            associations.set(&dotted, PROG_ID)?;
            Key::create(&format!(r"{}\OpenWithProgids", extension_key(extension)))?
                .set_empty(PROG_ID)?;
            let key = Key::create(&extension_key(extension))?;
            match key.get("") {
                Some(current) if current == PROG_ID => {}
                Some(current) if !current.is_empty() => {
                    key.set(PREVIOUS, &current)?;
                    key.set("", PROG_ID)?;
                }
                _ => key.set("", PROG_ID)?,
            }
        } else {
            unregister_extension(extension)?;
        }
    }
    notify();
    Ok(())
}

fn unregister_extension(extension: &str) -> io::Result<()> {
    let dotted = format!(".{extension}");
    let path = extension_key(extension);
    delete_value(&format!(r"{path}\OpenWithProgids"), PROG_ID)?;
    delete_value(&format!(r"{CAPABILITIES}\FileAssociations"), &dotted)?;
    if let Some(key) = Key::open(&path) {
        let previous = key.get(PREVIOUS);
        if key.get("").as_deref() == Some(PROG_ID) {
            drop(key);
            match previous {
                Some(previous) => Key::create(&path)?.set("", &previous)?,
                None => delete_value(&path, "")?,
            }
        }
        delete_value(&path, PREVIOUS)?;
    }
    Ok(())
}

/// Removes everything [`register`] wrote, for the extensions in `all`.
pub fn unregister(exe: &Path, all: &[&str]) -> io::Result<()> {
    for extension in all {
        unregister_extension(extension)?;
    }
    if let Some(name) = exe.file_name() {
        delete_tree(&format!(
            r"{CLASSES}\Applications\{}",
            name.to_string_lossy()
        ))?;
    }
    delete_tree(&format!(r"{CLASSES}\{PROG_ID}"))?;
    delete_value(REGISTERED_APPS, APP_NAME)?;
    // Holds only the capabilities (settings are files).
    delete_tree(r"Software\CuneiEdit")?;
    notify();
    Ok(())
}

/// For each extension: whether CuneiEdit is registered for it.
pub fn registered(all: &[&str]) -> Vec<bool> {
    all.iter()
        .map(|extension| {
            Key::open(&format!(r"{}\OpenWithProgids", extension_key(extension)))
                .is_some_and(|key| key.has(PROG_ID))
        })
        .collect()
}

/// The program that opens files with `extension` on a double click, if any.
pub fn default_program(extension: &str) -> Option<PathBuf> {
    let dotted = wide_nul(&format!(".{extension}"));
    let verb = wide_nul("open");
    let mut buffer = vec![0u16; MAX_VALUE];
    let mut len = buffer.len() as u32;
    // SAFETY: both strings are NUL-terminated; `buffer` holds `len` UTF-16 units.
    let result = unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_EXECUTABLE,
            dotted.as_ptr(),
            verb.as_ptr(),
            buffer.as_mut_ptr(),
            &mut len,
        )
    };
    if result != 0 {
        return None;
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(PathBuf::from(String::from_utf16_lossy(&buffer[..end])))
}

/// Opens Windows' default apps settings on CuneiEdit's page.
pub fn open_default_apps() -> io::Result<()> {
    let target = wide_nul(&format!(
        "ms-settings:defaultapps?registeredAppUser={APP_NAME}"
    ));
    let verb = wide_nul("open");
    // SAFETY: both strings are NUL-terminated; the other pointers may be null.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success.
    if result as usize > 32 {
        Ok(())
    } else {
        Err(io::Error::other("ShellExecuteW failed"))
    }
}

/// Tells Explorer that file associations changed.
fn notify() {
    // SAFETY: this event takes no items.
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            ptr::null(),
            ptr::null(),
        )
    };
}
