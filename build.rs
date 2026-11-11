//! Puts the cube icon (`tools/design_icon.py`) in the Windows executable, so
//! Explorer and the taskbar show it. Without the Windows SDK the build goes on
//! with a warning: the window still shows its icon.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The newest `rc.exe` of the installed Windows SDKs.
fn resource_compiler() -> Option<PathBuf> {
    let root = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(root).ok()?.flatten().map(|entry| entry.path()).collect();
    versions.sort();
    versions.into_iter().rev().map(|version| version.join("x64").join("rc.exe")).find(|path| path.is_file())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon/polyhedron.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(compiler) = resource_compiler() else {
        println!("cargo:warning=rc.exe not found: the executable has no icon of its own");
        return;
    };
    let output = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let script = output.join("icon.rc");
    let icon = manifest.join("assets/icon/polyhedron.ico");
    std::fs::write(&script, format!("1 ICON \"{}\"\n", icon.display().to_string().replace('\\', "/"))).unwrap();
    let compiled = output.join("icon.res");
    let status = Command::new(&compiler).arg("/nologo").arg("/fo").arg(&compiled).arg(&script).status();
    match status {
        Ok(status) if status.success() => println!("cargo:rustc-link-arg-bins={}", compiled.display()),
        _ => println!("cargo:warning=rc.exe failed: the executable has no icon of its own"),
    }
}
