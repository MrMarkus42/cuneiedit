//! Checksums of text and files, as hex strings.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    pub const ALL: [Self; 4] = [Self::Md5, Self::Sha1, Self::Sha256, Self::Sha512];

    pub fn label(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha512 => "SHA-512",
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn digest<D: Digest>(data: &[u8]) -> String {
    hex(&D::digest(data))
}

/// Checksum of the UTF-8 bytes of `text`.
pub fn of_text(algorithm: Algorithm, text: &str) -> String {
    let data = text.as_bytes();
    match algorithm {
        Algorithm::Md5 => digest::<Md5>(data),
        Algorithm::Sha1 => digest::<Sha1>(data),
        Algorithm::Sha256 => digest::<Sha256>(data),
        Algorithm::Sha512 => digest::<Sha512>(data),
    }
}

fn of_reader<D: Digest>(mut reader: impl Read) -> io::Result<String> {
    let mut hasher = D::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

/// Checksum of a file's bytes, read in chunks so large files need little memory.
pub fn of_file(algorithm: Algorithm, path: &Path) -> io::Result<String> {
    let file = File::open(path)?;
    match algorithm {
        Algorithm::Md5 => of_reader::<Md5>(file),
        Algorithm::Sha1 => of_reader::<Sha1>(file),
        Algorithm::Sha256 => of_reader::<Sha256>(file),
        Algorithm::Sha512 => of_reader::<Sha512>(file),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_digests() {
        assert_eq!(
            of_text(Algorithm::Md5, "abc"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            of_text(Algorithm::Sha1, "abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            of_text(Algorithm::Sha256, "abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(of_text(Algorithm::Sha512, "abc").starts_with("ddaf35a193617aba"));
    }

    #[test]
    fn hashes_files() {
        let path = std::env::temp_dir().join(format!("pilcrow-hash-{}", std::process::id()));
        std::fs::write(&path, "abc").unwrap();
        assert_eq!(
            of_file(Algorithm::Md5, &path).unwrap(),
            of_text(Algorithm::Md5, "abc")
        );
        std::fs::remove_file(&path).unwrap();
        assert!(of_file(Algorithm::Md5, &path).is_err());
    }
}
