//! Update manifest: which version is current and which file to install on each platform.
//!
//! Format (`update.txt`, signed as a whole by `update.txt.sig`):
//!
//! ```text
//! format=1
//! version=0.2.0
//! notes=First line\nSecond line
//! asset=macos-aarch64 CuneiEdit-0.2.0-macos-arm64.zip 5226790 <sha256 in hex>
//! ```

use std::fmt;

/// Largest manifest that is downloaded.
pub const MAX_MANIFEST_SIZE: u64 = 64 * 1024;
/// Largest installation file that is downloaded.
pub const MAX_ASSET_SIZE: u64 = 512 * 1024 * 1024;
const FORMAT: &str = "1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// Parses `major.minor.patch`; anything else (pre-releases, a leading `v`) is rejected.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.').map(|part| {
            let digits =
                !part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| part.parse().ok()).flatten()
        });
        let version = Self(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }

    /// The version of this build.
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("the package version is major.minor.patch")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A file to install on one platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// `os-arch` as in [`platform`], e.g. `macos-aarch64`.
    pub platform: String,
    /// File name in the GitHub release.
    pub name: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub notes: String,
    pub assets: Vec<Asset>,
}

/// The platform of this build, as used in manifests.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Whether `name` is a plain file name that is safe in a URL and a path.
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != 2 * N || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

impl Manifest {
    /// Parses a manifest; `None` if anything is malformed, missing or repeated.
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != format!("format={FORMAT}") {
            return None;
        }
        let mut version = None;
        let mut notes = None;
        let mut assets: Vec<Asset> = Vec::new();
        for line in lines.filter(|line| !line.is_empty()) {
            let (key, value) = line.split_once('=')?;
            match key {
                "version" if version.is_none() => version = Some(Version::parse(value)?),
                "notes" if notes.is_none() => notes = Some(unescape(value)),
                "asset" => {
                    let mut fields = value.split(' ');
                    let asset = Asset {
                        platform: fields.next().filter(|p| is_safe_name(p))?.to_owned(),
                        name: fields.next().filter(|n| is_safe_name(n))?.to_owned(),
                        size: fields
                            .next()?
                            .parse()
                            .ok()
                            .filter(|&size| size <= MAX_ASSET_SIZE)?,
                        sha256: from_hex(fields.next()?)?,
                    };
                    if fields.next().is_some()
                        || assets.iter().any(|a| a.platform == asset.platform)
                    {
                        return None;
                    }
                    assets.push(asset);
                }
                _ => return None,
            }
        }
        Some(Self {
            version: version?,
            notes: notes.unwrap_or_default(),
            assets,
        })
    }

    pub fn to_text(&self) -> String {
        let mut text = format!(
            "format={FORMAT}\nversion={}\nnotes={}\n",
            self.version,
            escape(&self.notes)
        );
        for asset in &self.assets {
            text.push_str(&format!(
                "asset={} {} {} {}\n",
                asset.platform,
                asset.name,
                asset.size,
                to_hex(&asset.sha256)
            ));
        }
        text
    }

    /// The file for this platform, if the release has one.
    pub fn asset(&self) -> Option<&Asset> {
        let platform = platform();
        self.assets.iter().find(|asset| asset.platform == platform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(Version::parse("1.20.3"), Some(Version(1, 20, 3)));
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.x",
            "1.2.-3",
            "1.2.3-beta",
            " 1.2.3",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert!(Version::current() >= Version(0, 1, 0));
    }

    #[test]
    fn round_trips_manifests() {
        let manifest = Manifest {
            version: Version(0, 2, 0),
            notes: "Fixes\n- a \\ b\r".into(),
            assets: vec![Asset {
                platform: "macos-aarch64".into(),
                name: "CuneiEdit-0.2.0-macos-arm64.zip".into(),
                size: 42,
                sha256: [7; 32],
            }],
        };
        assert_eq!(Manifest::parse(&manifest.to_text()), Some(manifest));
    }

    #[test]
    fn rejects_malformed_manifests() {
        let hash = "ab".repeat(32);
        let good = format!("format=1\nversion=1.0.0\nasset=linux-x86_64 a.bin 1 {hash}\n");
        assert!(Manifest::parse(&good).is_some());
        for bad in [
            "version=1.0.0".to_owned(),
            "format=2\nversion=1.0.0".to_owned(),
            "format=1\nnotes=x".to_owned(),
            "format=1\nversion=1.0.0\nversion=2.0.0".to_owned(),
            "format=1\nversion=1.0.0\nunknown=1".to_owned(),
            format!("format=1\nversion=1.0.0\nasset=linux-x86_64 ../a 1 {hash}"),
            format!("format=1\nversion=1.0.0\nasset=linux-x86_64 a 1 {hash} extra"),
            format!("format=1\nversion=1.0.0\nasset=linux-x86_64 a 99999999999 {hash}"),
            "format=1\nversion=1.0.0\nasset=linux-x86_64 a 1 xyz".to_owned(),
            format!("{good}asset=linux-x86_64 b 1 {hash}\n"),
        ] {
            assert_eq!(Manifest::parse(&bad), None, "{bad}");
        }
    }

    #[test]
    fn checks_names() {
        assert!(is_safe_name("CuneiEdit-0.2.0-windows-x64.exe"));
        for bad in ["", "..", ".hidden", "a/b", "a\\b", "a b", "a\"b", "ä"] {
            assert!(!is_safe_name(bad), "{bad}");
        }
    }
}
