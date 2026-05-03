use embed_manifest::{embed_manifest, new_manifest};
use std::process::Command;

fn set_version(res: &mut winres::WindowsResource, major: u32, minor: u32, build: u32) {
    let file_ver = ((major as u64) << 48) | ((minor as u64) << 32) | (build as u64);
    let product_ver = file_ver;
    res.set_version_info(winres::VersionInfo::FILEVERSION, file_ver);
    res.set_version_info(winres::VersionInfo::PRODUCTVERSION, product_ver);
}

fn main() {
    let (major, minor, build) = (1, 0, 1);

    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("Megatops.UDPFwd")).expect("failed to embed manifest");

        let mut res = winres::WindowsResource::new();
        res.set_icon("resources/icon.ico");
        res.set("CompanyName", "Megatops Software");
        res.set("ProductName", "UDP Forwarder");
        res.set("FileDescription", "UDP Forwarder - Forward UDP packets to target IP:Port");
        res.set("LegalCopyright", "Copyright (c) 2026 Ding Zhaojie <zhaojie_ding@msn.com>");
        set_version(&mut res, major, minor, build);
        res.compile().ok();
    }

    let src_files = ["src/main.rs", "src/forwarder.rs"];
    for file in &src_files {
        let _ = Command::new("rustfmt").arg(file).output();
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/forwarder.rs");
    println!("cargo:rerun-if-changed=resources/icon.ico");
    println!("cargo:rerun-if-changed=resources/icon.rc");
}