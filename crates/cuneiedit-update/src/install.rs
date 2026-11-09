//! Installing a downloaded update in place and starting the new version.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::{Asset, MAX_ASSET_SIZE, from_hex};
use crate::{Error, NotInstallable, Release, matches_asset};

/// Command-line flag of the elevated helper that replaces the executable on Windows.
pub const APPLY_ARG: &str = "--cuneiedit-apply-update";

/// Whether updates can be installed on this platform at all.
pub(crate) fn supported() -> bool {
    cfg!(any(target_os = "macos", windows))
}

/// Whether an update can be installed without asking for administrator rights.
pub fn can_install_silently() -> bool {
    platform::target_dir().is_some_and(|dir| {
        let probe = dir.join(format!(".cuneiedit-write-test-{}", std::process::id()));
        let writable = fs::create_dir(&probe).is_ok();
        let _ = fs::remove_dir(&probe);
        writable
    })
}

/// Installs the downloaded `file` of `release` over the running copy. Returns what to start
/// for the new version; it takes effect after a restart.
pub fn install(file: &Path, release: &Release) -> Result<PathBuf, Error> {
    let asset = release
        .manifest
        .asset()
        .ok_or(Error::NotInstallable(NotInstallable::Platform))?;
    platform::install(file, asset)
}

/// Starts the installed new version, e.g. after this one has quit.
pub fn relaunch(path: &Path) -> io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("/usr/bin/open");
        command.arg("-n").arg(path);
        command
    } else {
        Command::new(path)
    };
    command.spawn().map(drop)
}

/// Removes what an earlier update left behind.
pub fn cleanup() {
    if cfg!(windows)
        && let Ok(exe) = env::current_exe()
    {
        let _ = fs::remove_file(old_path(&exe));
    }
}

pub(crate) fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

fn old_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

/// Replaces the executable `target` with `bytes`. The running executable can be renamed but
/// not overwritten, so the old one is moved aside and removed on the next start.
fn replace(bytes: &[u8], target: &Path) -> io::Result<()> {
    let old = old_path(target);
    let _ = fs::remove_file(&old);
    fs::rename(target, &old)?;
    if let Err(err) = fs::write(target, bytes) {
        let _ = fs::remove_file(target);
        let _ = fs::rename(&old, target);
        return Err(err);
    }
    Ok(())
}

/// Runs the elevated helper mode if the process was started with [`APPLY_ARG`]; returns the
/// exit code. The helper only replaces the running executable, and only with a file that
/// matches the size and SHA-256 it was given.
pub fn apply_from_args() -> Option<i32> {
    let args: Vec<OsString> = env::args_os().collect();
    if args.get(1)? != APPLY_ARG {
        return None;
    }
    Some(match apply(&args[2..]) {
        Ok(()) => 0,
        Err(_) => 1,
    })
}

fn apply(args: &[OsString]) -> Result<(), Error> {
    let invalid = || Error::Io(io::ErrorKind::InvalidInput.into());
    let [source, target, size, hash] = args else {
        return Err(invalid());
    };
    let asset = Asset {
        platform: String::new(),
        name: String::new(),
        size: size
            .to_str()
            .and_then(|size| size.parse().ok())
            .filter(|&size| size <= MAX_ASSET_SIZE)
            .ok_or_else(invalid)?,
        sha256: hash.to_str().and_then(from_hex).ok_or_else(invalid)?,
    };
    let target = PathBuf::from(target);
    if target.canonicalize()? != env::current_exe()?.canonicalize()? {
        return Err(invalid());
    }
    let bytes = fs::read(source)?;
    if !matches_asset(&bytes, &asset) {
        return Err(Error::Checksum);
    }
    replace(&bytes, &target)?;
    Ok(())
}

#[cfg(target_os = "macos")]
mod platform {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::{self, Command};

    use crate::manifest::Asset;
    use crate::{Error, NotInstallable};

    /// Moves the old bundle aside, puts the new one in place and removes the old one; puts the
    /// old one back if that fails. Paths are passed as arguments and quoted by AppleScript.
    const ADMIN_SCRIPT: &str = r#"on run argv
    set src to quoted form of item 1 of argv
    set dst to quoted form of item 2 of argv
    set old to quoted form of item 3 of argv
    do shell script "/bin/mv " & dst & " " & old & " && { /usr/bin/ditto " & src & " " & dst & " || { /bin/rm -rf " & dst & "; /bin/mv " & old & " " & dst & "; exit 1; }; } && /bin/rm -rf " & old with administrator privileges
end run"#;

    /// The `.app` bundle this process runs from.
    fn bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
        let macos = exe.parent()?;
        let bundle = macos.parent()?.parent()?;
        (macos.ends_with("Contents/MacOS") && bundle.extension()? == "app")
            .then(|| bundle.to_path_buf())
    }

    /// The folder the bundle is replaced in, if this copy can be updated at all.
    pub fn target_dir() -> Option<PathBuf> {
        let bundle = bundle()?;
        let path = bundle.to_string_lossy();
        // Disk images are read-only; translocated apps run from a random read-only copy.
        if path.starts_with("/Volumes/") || path.contains("/AppTranslocation/") {
            return None;
        }
        bundle.parent().map(Path::to_path_buf)
    }

    pub fn install(zip: &Path, _asset: &Asset) -> Result<PathBuf, Error> {
        let location = || Error::NotInstallable(NotInstallable::Location);
        let parent = target_dir().ok_or_else(location)?;
        let bundle = bundle().ok_or_else(location)?;
        let stage = parent.join(format!(".cuneiedit-update-{}", process::id()));
        match fs::create_dir(&stage) {
            Ok(()) => {
                let result = replace_in_place(zip, &stage, &bundle);
                let _ = fs::remove_dir_all(&stage);
                result?;
            }
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                replace_as_admin(zip, &bundle)?;
            }
            Err(err) => return Err(err.into()),
        }
        Ok(bundle)
    }

    /// Unpacks the update archive into `dir`; returns the new bundle.
    fn unpack(zip: &Path, dir: &Path) -> Result<PathBuf, Error> {
        let status = Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(zip)
            .arg(dir)
            .status()?;
        let app = dir.join("CuneiEdit.app");
        if !status.success() || !app.join("Contents/MacOS/CuneiEdit").is_file() {
            return Err(Error::Checksum);
        }
        Ok(app)
    }

    fn old_bundle(bundle: &Path) -> PathBuf {
        bundle.with_file_name(format!(".CuneiEdit-old-{}.app", process::id()))
    }

    /// Next to the bundle, so both renames stay on one volume.
    fn replace_in_place(zip: &Path, stage: &Path, bundle: &Path) -> Result<(), Error> {
        let new = unpack(zip, stage)?;
        let old = old_bundle(bundle);
        fs::rename(bundle, &old)?;
        if let Err(err) = fs::rename(&new, bundle) {
            let _ = fs::rename(&old, bundle);
            return Err(err.into());
        }
        let _ = fs::remove_dir_all(&old);
        Ok(())
    }

    /// The folder is not writable (e.g. `/Applications` for a standard user): macOS asks for
    /// an administrator's password.
    fn replace_as_admin(zip: &Path, bundle: &Path) -> Result<(), Error> {
        let stage = zip.with_extension("unpacked");
        let _ = fs::remove_dir_all(&stage);
        fs::create_dir(&stage)?;
        let result = unpack(zip, &stage).and_then(|new| {
            let status = Command::new("/usr/bin/osascript")
                .arg("-e")
                .arg(ADMIN_SCRIPT)
                .arg(&new)
                .arg(bundle)
                .arg(old_bundle(bundle))
                .status()?;
            if status.success() {
                Ok(())
            } else {
                Err(Error::Denied)
            }
        });
        let _ = fs::remove_dir_all(&stage);
        result
    }
}

#[cfg(windows)]
mod platform {
    use std::env;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{APPLY_ARG, hide_window, replace};
    use crate::manifest::{Asset, to_hex};
    use crate::{Error, matches_asset};

    pub fn target_dir() -> Option<PathBuf> {
        env::current_exe().ok()?.parent().map(Path::to_path_buf)
    }

    pub fn install(file: &Path, asset: &Asset) -> Result<PathBuf, Error> {
        let target = env::current_exe()?;
        let bytes = std::fs::read(file)?;
        if !matches_asset(&bytes, asset) {
            return Err(Error::Checksum);
        }
        match replace(&bytes, &target) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                elevate(&target, file, asset)?;
            }
            Err(err) => return Err(err.into()),
        }
        Ok(target)
    }

    /// Runs the installed executable (in a folder only administrators can change) as the
    /// elevated helper; Windows shows the UAC prompt. The helper checks the file itself.
    fn elevate(target: &Path, file: &Path, asset: &Asset) -> Result<(), Error> {
        let target = target.to_string_lossy();
        let file = file.to_string_lossy();
        // Windows paths cannot contain `"`, so double quotes delimit them safely.
        if target.contains('"') || file.contains('"') {
            return Err(Error::Io(io::ErrorKind::InvalidInput.into()));
        }
        let arguments = format!(
            "{APPLY_ARG} \"{file}\" \"{target}\" {} {}",
            asset.size,
            to_hex(&asset.sha256)
        );
        let quote = |text: &str| format!("'{}'", text.replace('\'', "''"));
        let script = format!(
            "$p = Start-Process -FilePath {} -ArgumentList {} -Verb RunAs -Wait -PassThru; \
             exit $p.ExitCode",
            quote(&target),
            quote(&arguments)
        );
        let root = env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let powershell = Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let mut command = Command::new(powershell);
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        hide_window(&mut command);
        if command.status()?.success() {
            Ok(())
        } else {
            Err(Error::Denied)
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use std::path::{Path, PathBuf};

    use crate::manifest::Asset;
    use crate::{Error, NotInstallable};

    pub fn target_dir() -> Option<PathBuf> {
        None
    }

    pub fn install(_file: &Path, _asset: &Asset) -> Result<PathBuf, Error> {
        Err(Error::NotInstallable(NotInstallable::Platform))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha256;

    #[test]
    fn replaces_files_and_keeps_the_old_one_aside() {
        let dir = env::temp_dir().join(format!("cuneiedit-replace-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("cuneiedit.exe");
        fs::write(&target, b"old").unwrap();
        replace(b"new", &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(fs::read(old_path(&target)).unwrap(), b"old");
        replace(b"newer", &target).unwrap();
        assert_eq!(fs::read(old_path(&target)).unwrap(), b"new");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn helper_only_replaces_the_running_executable() {
        let dir = env::temp_dir().join(format!("cuneiedit-apply-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("new.exe");
        let target = dir.join("other.exe");
        fs::write(&source, b"new").unwrap();
        fs::write(&target, b"old").unwrap();
        let hash = crate::manifest::to_hex(&sha256(b"new"));
        let args = |target: &Path, size: &str, hash: &str| -> Vec<OsString> {
            vec![
                source.clone().into(),
                target.into(),
                size.into(),
                hash.into(),
            ]
        };
        assert!(
            apply(&args(&target, "3", &hash)).is_err(),
            "not the running exe"
        );
        assert_eq!(fs::read(&target).unwrap(), b"old");
        let exe = env::current_exe().unwrap();
        assert!(matches!(
            apply(&args(&exe, "3", &"00".repeat(32))),
            Err(Error::Checksum)
        ));
        assert!(apply(&args(&exe, "x", &hash)).is_err());
        assert!(apply(&args(&exe, "3", &hash)[..3]).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
