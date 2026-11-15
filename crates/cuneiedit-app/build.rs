//! Embeds the application icon into the Windows executable.

#[path = "src/app_icon.rs"]
#[allow(dead_code)]
mod app_icon;
#[path = "src/icon_files.rs"]
#[allow(dead_code)]
mod icon_files;

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

fn main() {
    println!("cargo::rerun-if-changed=src/app_icon.rs");
    println!("cargo::rerun-if-changed=src/icon_files.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let images: Vec<(u32, Vec<u8>)> = [16, 24, 32, 48, 64, 256]
        .into_iter()
        .map(|size| (size, icon_files::png(size, &app_icon::rgba(size))))
        .collect();
    let dir = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo"));
    let res = dir.join("icon.res");
    fs::write(&res, icon_files::windows_res(&images)).expect("OUT_DIR is writable");
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // The MSVC linker takes compiled resource files as input.
        println!("cargo::rustc-link-arg-bins={}", res.display());
        return;
    }
    // GNU ld needs an object file: MinGW's windres converts the resource file (build time
    // only, with fixed arguments).
    let object = dir.join("icon.o");
    for windres in ["x86_64-w64-mingw32-windres", "windres"] {
        let converted = Command::new(windres)
            .args(["--input-format=res", "--output-format=coff", "-i"])
            .arg(&res)
            .arg("-o")
            .arg(&object)
            .status()
            .is_ok_and(|status| status.success());
        if converted {
            println!("cargo::rustc-link-arg-bins={}", object.display());
            return;
        }
    }
    println!("cargo::warning=windres not found: the executable gets no icon");
}
