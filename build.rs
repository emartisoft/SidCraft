// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

//! Build script: embeds the application icon into the Windows executable.
//!
//! On Windows the icon is compiled into the `.exe` as a resource so that
//! Explorer, the task bar and Alt+Tab show it. On other platforms the script
//! does nothing; the window icon is set at runtime from `icon/sidcraft.png`
//! (see `src/app_icon.rs`).
//!
//! The decision is made from `CARGO_CFG_TARGET_*`, not from `#[cfg(windows)]`:
//! a build script is compiled for the *host*, so `#[cfg(windows)]` would
//! answer "is the build machine Windows?" instead of "is the binary being
//! built for Windows?" — and would embed Windows resources into a Linux or
//! macOS cross-compile.

fn main() {
    println!("cargo:rerun-if-changed=icon/sidcraft.ico");
    println!("cargo:rerun-if-changed=icon/sidcraft.png");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env_var("CARGO_CFG_TARGET_OS");
    let target_env = env_var("CARGO_CFG_TARGET_ENV");

    // The icon is a Windows PE resource; other platforms get it at runtime.
    if target_os == "windows" && (target_env == "msvc" || target_env == "gnu") {
        embed_windows_icon();
    }
}

/// Reads a Cargo environment variable, treating a missing one as empty.
fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// Compiles `icon/sidcraft.ico` into the PE resource section.
fn embed_windows_icon() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("icon/sidcraft.ico");
    res.set("FileDescription", "SidCraft - SID Player");
    res.set("ProductName", "SidCraft");
    res.set("CompanyName", "Emarti");
    res.set("LegalCopyright", "Copyright (c) 2026 Emarti");
    res.set("OriginalFilename", "sidcraft.exe");
    if let Err(err) = res.compile() {
        // A missing icon must never break the build.
        println!("cargo:warning=sidcraft: could not embed the icon: {err}");
    }
}
