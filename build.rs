// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Ding Zhaojie <zhaojie_ding@msn.com>

//! Build script — embeds a Windows manifest and icon resources into the
//! executable and declares `cargo:rerun-if-changed` directives.

use embed_manifest::{embed_manifest, new_manifest};

/// Application version components.
const VERSION_MAJOR: u32 = 1;
const VERSION_MINOR: u32 = 0;
const VERSION_BUILD: u32 = 1;

/// Encodes `(major, minor, build)` as a 64-bit Windows version integer.
///
/// Layout: `[16-bit major][16-bit minor][32-bit build]`.
fn encode_version(major: u32, minor: u32, build: u32) -> u64 {
    ((major as u64) << 48) | ((minor as u64) << 32) | (build as u64)
}

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    // Embed a common-controls v6 manifest for modern widget styles
    embed_manifest(new_manifest("Megatops.UDPFwd")).expect("Failed to embed manifest");

    // Embed the application icon and version info
    let file_ver = encode_version(VERSION_MAJOR, VERSION_MINOR, VERSION_BUILD);
    let mut res = winres::WindowsResource::new();
    res.set_icon("resources/icon.ico");
    res.set("CompanyName", "Megatops Software");
    res.set("ProductName", "UDP Forwarder");
    res.set(
        "FileDescription",
        "UDP Forwarder - Forward UDP packets to target IP:Port",
    );
    res.set(
        "LegalCopyright",
        "Copyright (c) 2026 Ding Zhaojie <zhaojie_ding@msn.com>",
    );
    res.set_version_info(winres::VersionInfo::FILEVERSION, file_ver);
    res.set_version_info(winres::VersionInfo::PRODUCTVERSION, file_ver);
    res.compile().expect("Failed to compile resources");

    // Re-run this script when any of these files change
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/forwarder.rs");
    println!("cargo:rerun-if-changed=resources/icon.ico");
}
