//! Embeds the application icon into the executable.
//!
//! Windows reads an executable's icon from its resource section, so the `.ico` has to
//! be compiled by the resource compiler that ships with the Windows SDK and linked
//! into the binary. Linking a `.res` file is just an argument to the linker.
//!
//! If the resource compiler cannot be found the build still succeeds, with a warning
//! and without an icon, so a machine without the SDK is not blocked.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=win-quran.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    let resource = Path::new(&out_dir).join("win-quran.res");

    let Some(compiler) = find_resource_compiler() else {
        println!("cargo:warning=rc.exe not found, so the executable has no icon");
        return;
    };

    let compiled = Command::new(&compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg("win-quran.rc")
        .status();

    match compiled {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg-bins={}", resource.display());
        }
        _ => println!("cargo:warning=the icon resource could not be compiled"),
    }
}

/// Finds `rc.exe` in the newest Windows SDK installed, for the host architecture.
fn find_resource_compiler() -> Option<PathBuf> {
    let kits = Path::new("C:/Program Files (x86)/Windows Kits/10/bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(kits)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    versions.sort();
    versions.reverse();
    for version in versions {
        let candidate = version.join("x64").join("rc.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
