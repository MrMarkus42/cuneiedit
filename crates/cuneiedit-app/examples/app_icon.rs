//! Writes the application icon as PNG files for a macOS `.iconset` folder:
//! `cargo run -p cuneiedit-app --example app_icon -- <folder>`.

#[path = "../src/app_icon.rs"]
mod app_icon;
#[path = "../src/icon_files.rs"]
#[allow(dead_code)]
mod icon_files;

use std::path::PathBuf;
use std::{env, fs, io};

use icon_files::png;

fn main() -> io::Result<()> {
    let dir = PathBuf::from(env::args_os().nth(1).ok_or(io::ErrorKind::InvalidInput)?);
    fs::create_dir_all(&dir)?;
    for size in [16, 32, 128, 256, 512] {
        for scale in [1, 2] {
            let pixels = size * scale;
            let suffix = if scale == 2 { "@2x" } else { "" };
            let path = dir.join(format!("icon_{size}x{size}{suffix}.png"));
            fs::write(path, png(pixels, &app_icon::rgba(pixels)))?;
        }
    }
    Ok(())
}
