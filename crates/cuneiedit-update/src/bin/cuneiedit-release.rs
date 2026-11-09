//! Release tool (not shipped): creates the signing key, writes and signs `update.txt`.
//!
//! ```text
//! cuneiedit-release keygen <secret-key-file>
//! cuneiedit-release manifest <version> <notes-file> [<platform> <file>]... > update.txt
//! CUNEIEDIT_UPDATE_KEY=<secret key> cuneiedit-release sign update.txt
//! ```

use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::ExitCode;
use std::{env, fs, io};

use cuneiedit_update::manifest::{Asset, Manifest, Version, from_hex, is_safe_name, to_hex};
use cuneiedit_update::{SIGNATURE_NAME, sha256, verify};
use ed25519_compact::{KeyPair, Seed};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("keygen") if args.len() == 2 => keygen(Path::new(&args[1])),
        Some("manifest") if args.len() >= 3 && args.len() % 2 == 1 => manifest(&args[1..]),
        Some("sign") if args.len() == 2 => sign(Path::new(&args[1])),
        _ => Err(
            "usage: cuneiedit-release keygen <file> | manifest <version> <notes-file> \
                  [<platform> <file>]... | sign <file>"
                .to_owned(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("cuneiedit-release: {message}");
            ExitCode::FAILURE
        }
    }
}

fn io_error(err: io::Error) -> String {
    err.to_string()
}

/// Writes a new secret key (hex) to `path`, readable only by the owner, and prints the
/// public key for `PUBLIC_KEYS`.
fn keygen(path: &Path) -> Result<(), String> {
    let mut seed = [0u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut seed))
        .map_err(io_error)?;
    let pair = KeyPair::from_seed(Seed::new(seed));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(to_hex(&seed).as_bytes()))
        .map_err(io_error)?;
    println!("{}", to_hex(pair.pk.as_ref()));
    Ok(())
}

fn manifest(args: &[String]) -> Result<(), String> {
    let version = Version::parse(&args[0]).ok_or("the version must be major.minor.patch")?;
    let notes = fs::read_to_string(&args[1]).map_err(io_error)?;
    let mut assets = Vec::new();
    for pair in args[2..].chunks(2) {
        let path = Path::new(&pair[1]);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| is_safe_name(name))
            .ok_or_else(|| format!("unsuitable file name: {}", pair[1]))?;
        if !is_safe_name(&pair[0]) {
            return Err(format!("unsuitable platform: {}", pair[0]));
        }
        let bytes = fs::read(path).map_err(io_error)?;
        assets.push(Asset {
            platform: pair[0].clone(),
            name: name.to_owned(),
            size: bytes.len() as u64,
            sha256: sha256(&bytes),
        });
    }
    let manifest = Manifest {
        version,
        notes: notes.trim().to_owned(),
        assets,
    };
    let text = manifest.to_text();
    if Manifest::parse(&text).as_ref() != Some(&manifest) {
        return Err("the manifest does not read back".to_owned());
    }
    print!("{text}");
    Ok(())
}

/// Signs `path` with the key in `CUNEIEDIT_UPDATE_KEY` into `update.txt.sig` next to it, and
/// checks that the app would accept the signature.
fn sign(path: &Path) -> Result<(), String> {
    let key = env::var("CUNEIEDIT_UPDATE_KEY").map_err(|_| "CUNEIEDIT_UPDATE_KEY is not set")?;
    let seed = from_hex::<32>(&key).ok_or("CUNEIEDIT_UPDATE_KEY is not a 32-byte hex key")?;
    let pair = KeyPair::from_seed(Seed::new(seed));
    let message = fs::read(path).map_err(io_error)?;
    let signature = to_hex(pair.sk.sign(&message, None).as_ref());
    if !verify(&message, &signature) {
        return Err("the key is not one of PUBLIC_KEYS; the app would reject it".to_owned());
    }
    let out = path.with_file_name(SIGNATURE_NAME);
    fs::write(&out, signature + "\n").map_err(io_error)?;
    println!("{}", out.display());
    Ok(())
}
