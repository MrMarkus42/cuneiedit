//! Signed updates from the project's GitHub releases.
//!
//! Every release carries `update.txt` (see [`manifest`]) and its Ed25519 signature
//! `update.txt.sig`, made by the release workflow with a key that is not stored on GitHub
//! in plain form. The manifest names the file for each platform with its size and SHA-256,
//! so one signature covers everything. Nothing is installed unless the signature is valid
//! for one of [`PUBLIC_KEYS`], the version is newer than this build and the downloaded file
//! matches the manifest.
//!
//! Downloads use the operating system's `curl` (HTTPS only, also for redirects), so TLS and
//! its certificates are maintained by the system.

pub mod manifest;

mod install;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ed25519_compact::{PublicKey, Signature};
use sha2::{Digest, Sha256};

pub use install::{apply_from_args, can_install_silently, cleanup, install, relaunch};
pub use manifest::{Asset, Manifest, Version};

/// GitHub repository the releases come from.
pub const REPOSITORY: &str = "MrMarkus42/cuneiedit";
/// Keys whose signatures are accepted. A new key is added here one release before the old
/// one is retired.
pub const PUBLIC_KEYS: &[&str] =
    &["9f7e23c8353b34d08ce9274447149fc0579e336733621223fcf3d933455a833b"];
pub const MANIFEST_NAME: &str = "update.txt";
pub const SIGNATURE_NAME: &str = "update.txt.sig";

#[derive(Debug)]
pub enum Error {
    /// `curl` failed: its exit code and message.
    Download(Option<i32>, String),
    /// The manifest is not signed with a known key.
    Signature,
    /// The manifest could not be read.
    Manifest,
    /// The downloaded file does not match the manifest.
    Checksum,
    /// Updates cannot be installed here; see [`NotInstallable`].
    NotInstallable(NotInstallable),
    /// Installing needed administrator rights and they were not given.
    Denied,
    Io(io::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotInstallable {
    /// No file for this platform in the release, or no installer for this platform.
    Platform,
    /// Not running from an installed app, e.g. from a disk image or a development build.
    Location,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Download(code, message) => match code {
                Some(code) => write!(f, "download failed (curl {code}): {message}"),
                None => write!(f, "download failed: {message}"),
            },
            Self::Signature => f.write_str("the update is not signed correctly"),
            Self::Manifest => f.write_str("the update information is damaged"),
            Self::Checksum => f.write_str("the downloaded file is damaged"),
            Self::NotInstallable(NotInstallable::Platform) => {
                f.write_str("there is no installable file for this system")
            }
            Self::NotInstallable(NotInstallable::Location) => {
                f.write_str("this copy of CuneiEdit cannot be updated in place")
            }
            Self::Denied => f.write_str("administrator rights were not given"),
            Self::Io(err) => err.fmt(f),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// A newer release, with its verified manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub manifest: Manifest,
}

impl Release {
    pub fn version(&self) -> Version {
        self.manifest.version
    }

    pub fn notes(&self) -> &str {
        &self.manifest.notes
    }

    /// Whether this release can be installed on this platform.
    pub fn installable(&self) -> bool {
        self.manifest.asset().is_some() && install::supported()
    }

    pub fn page_url(&self) -> String {
        format!(
            "https://github.com/{REPOSITORY}/releases/tag/v{}",
            self.manifest.version
        )
    }
}

fn curl() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        Path::new(&root).join("System32").join("curl.exe")
    } else {
        PathBuf::from("/usr/bin/curl")
    }
}

/// Downloads `url` into `dest`. `Ok(false)` if the server has no such file (HTTP 404).
fn fetch(url: &str, dest: &Path, max_size: u64) -> Result<bool, Error> {
    let mut command = Command::new(curl());
    command
        // Ignore ~/.curlrc, so only these options apply.
        .arg("--disable")
        .args(["--proto", "=https", "--proto-redir", "=https", "--tlsv1.2"])
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args([
            "--max-redirs",
            "5",
            "--connect-timeout",
            "20",
            "--max-time",
            "900",
        ])
        .arg("--max-filesize")
        .arg(max_size.to_string())
        .arg("--write-out")
        .arg("%{http_code}")
        .arg("--output")
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null());
    install::hide_window(&mut command);
    let output = command
        .output()
        .map_err(|err| Error::Download(None, err.to_string()))?;
    if output.status.success() {
        return Ok(true);
    }
    let _ = fs::remove_file(dest);
    if String::from_utf8_lossy(&output.stdout).trim() == "404" {
        return Ok(false);
    }
    let message: String = String::from_utf8_lossy(&output.stderr)
        .trim()
        .chars()
        .take(300)
        .collect();
    Err(Error::Download(output.status.code(), message))
}

/// Whether `signature` (hex) is a valid signature of `message` by one of [`PUBLIC_KEYS`].
pub fn verify(message: &[u8], signature: &str) -> bool {
    let Some(signature) = manifest::from_hex::<64>(signature) else {
        return false;
    };
    let signature = Signature::new(signature);
    PUBLIC_KEYS.iter().any(|key| {
        manifest::from_hex::<32>(key)
            .and_then(|key| PublicKey::from_slice(&key).ok())
            .is_some_and(|key| key.verify(message, &signature).is_ok())
    })
}

/// Reads the verified manifest of `message` and `signature`.
pub fn verified_manifest(message: &[u8], signature: &str) -> Result<Manifest, Error> {
    if !verify(message, signature) {
        return Err(Error::Signature);
    }
    std::str::from_utf8(message)
        .ok()
        .and_then(Manifest::parse)
        .ok_or(Error::Manifest)
}

/// Looks for a release newer than this build. `dir` is a private folder for downloads.
pub fn check(dir: &Path) -> Result<Option<Release>, Error> {
    fs::create_dir_all(dir)?;
    let base = format!("https://github.com/{REPOSITORY}/releases/latest/download");
    let manifest_path = dir.join(MANIFEST_NAME);
    let signature_path = dir.join(SIGNATURE_NAME);
    let found = fetch(
        &format!("{base}/{MANIFEST_NAME}"),
        &manifest_path,
        manifest::MAX_MANIFEST_SIZE,
    )? && fetch(&format!("{base}/{SIGNATURE_NAME}"), &signature_path, 1024)?;
    if !found {
        // The latest release has no update information (e.g. it predates updates).
        return Ok(None);
    }
    let message = fs::read(&manifest_path)?;
    let signature = fs::read_to_string(&signature_path).unwrap_or_default();
    let manifest = verified_manifest(&message, &signature)?;
    Ok((manifest.version > Version::current()).then_some(Release { manifest }))
}

/// Downloads the file for this platform into `dir` and checks it against the manifest.
pub fn download(release: &Release, dir: &Path) -> Result<PathBuf, Error> {
    let asset = release
        .manifest
        .asset()
        .ok_or(Error::NotInstallable(NotInstallable::Platform))?;
    fs::create_dir_all(dir)?;
    let path = dir.join(&asset.name);
    let url = format!(
        "https://github.com/{REPOSITORY}/releases/download/v{}/{}",
        release.manifest.version, asset.name
    );
    if !fetch(&url, &path, asset.size)? {
        return Err(Error::Download(Some(22), "404".into()));
    }
    if !matches_asset(&fs::read(&path)?, asset) {
        let _ = fs::remove_file(&path);
        return Err(Error::Checksum);
    }
    Ok(path)
}

/// Whether `bytes` are exactly the file the manifest describes.
pub fn matches_asset(bytes: &[u8], asset: &Asset) -> bool {
    bytes.len() as u64 == asset.size && sha256(bytes) == asset.sha256
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::{KeyPair, Seed};

    #[test]
    fn verifies_signatures_only_from_known_keys() {
        let message = b"format=1\nversion=9.9.9\n";
        let stranger = KeyPair::from_seed(Seed::new([3; 32]));
        let forged = manifest::to_hex(stranger.sk.sign(message, None).as_ref());
        assert!(!verify(message, &forged));
        assert!(!verify(message, "00"));
        assert!(!verify(message, &"zz".repeat(64)));
        assert!(matches!(
            verified_manifest(message, &forged),
            Err(Error::Signature)
        ));
    }

    #[test]
    fn public_keys_are_valid() {
        for key in PUBLIC_KEYS {
            let key = manifest::from_hex::<32>(key).expect("hex");
            assert!(PublicKey::from_slice(&key).is_ok());
            assert_ne!(key, [0; 32]);
        }
    }

    #[test]
    fn checks_downloaded_files() {
        let bytes = b"new cuneiedit";
        let asset = Asset {
            platform: manifest::platform(),
            name: "cuneiedit".into(),
            size: bytes.len() as u64,
            sha256: sha256(bytes),
        };
        assert!(matches_asset(bytes, &asset));
        assert!(!matches_asset(b"new pilcrox", &asset));
        assert!(!matches_asset(b"new cuneiedit!", &asset));
    }
}
