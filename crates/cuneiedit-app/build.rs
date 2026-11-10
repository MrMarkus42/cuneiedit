//! Embeds the application icon into the Windows executable (MSVC targets).

#[path = "src/app_icon.rs"]
#[allow(dead_code)]
mod app_icon;
#[path = "src/icon_files.rs"]
#[allow(dead_code)]
mod icon_files;

use std::path::PathBuf;
use std::{env, fs};

fn main() {
    println!("cargo::rerun-if-changed=src/app_icon.rs");
    println!("cargo::rerun-if-changed=src/icon_files.rs");
    let windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !(windows && msvc) {
        return;
    }
    let images: Vec<(u32, Vec<u8>)> = [16, 24, 32, 48, 64, 256]
        .into_iter()
        .map(|size| (size, icon_files::png(size, &app_icon::rgba(size))))
        .collect();
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo")).join("icon.res");
    fs::write(&out, icon_files::windows_res(&images)).expect("OUT_DIR is writable");
    // The MSVC linker takes compiled resource files as input.
    println!("cargo::rustc-link-arg-bins={}", out.display());
}
