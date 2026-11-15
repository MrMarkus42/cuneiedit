//! Writes the application icon as PNG files for a macOS `.iconset` folder, or as a Windows
//! icon file (for the installer):
//! `cargo run -p cuneiedit-app --example app_icon -- <folder>` or `-- <file>.ico`.

#[path = "../src/app_icon.rs"]
mod app_icon;
#[path = "../src/icon_files.rs"]
#[allow(dead_code)]
mod icon_files;

use std::path::PathBuf;
use std::{env, fs, io};

use icon_files::png;

fn main() -> io::Result<()> {
    let target = PathBuf::from(env::args_os().nth(1).ok_or(io::ErrorKind::InvalidInput)?);
    if target.extension().is_some_and(|ext| ext == "ico") {
        let images: Vec<(u32, Vec<u8>)> = [16, 24, 32, 48, 64, 256]
            .into_iter()
            .map(|size| (size, png(size, &app_icon::rgba(size))))
            .collect();
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        return fs::write(target, icon_files::ico(&images));
    }
    fs::create_dir_all(&target)?;
    for size in [16, 32, 128, 256, 512] {
        for scale in [1, 2] {
            let pixels = size * scale;
            let suffix = if scale == 2 { "@2x" } else { "" };
            let path = target.join(format!("icon_{size}x{size}{suffix}.png"));
            fs::write(path, png(pixels, &app_icon::rgba(pixels)))?;
        }
    }
    Ok(())
}
