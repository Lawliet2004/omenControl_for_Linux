use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output directory"))
        .join("omen.gresource");
    let status = Command::new("glib-compile-resources")
        .args([
            "--sourcedir=assets",
            "assets/omen.gresource.xml",
            "--target",
        ])
        .arg(output)
        .status()
        .expect("glib-compile-resources is required (provided by GLib development tools)");
    assert!(status.success(), "could not compile bundled icon resources");
}
