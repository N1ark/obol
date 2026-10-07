//! Detect the rustc Obol is compiled with, so that the few rustc API differences between the
//! supported toolchains can be selected with `#[cfg(rustc_1_98)]` (see
//! `src/bin/obol-driver/rustc_compat.rs`). The rest of the code base targets the newest supported
//! toolchain.

use std::{env, process::Command};

fn main() {
    println!("cargo::rerun-if-env-changed=RUSTC");
    println!("cargo::rustc-check-cfg=cfg(rustc_1_98)");

    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(&rustc)
        .arg("--version")
        .output()
        .unwrap_or_else(|e| panic!("cannot run `{rustc} --version`: {e}"));
    let version = String::from_utf8_lossy(&output.stdout);
    // `rustc 1.98.1 (48a229cea 2026-09-01)`
    let minor: u32 = version
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.split('.').nth(1))
        .and_then(|m| m.parse().ok())
        .unwrap_or_else(|| panic!("cannot parse the rustc version: {version}"));
    if minor == 98 {
        println!("cargo::rustc-cfg=rustc_1_98");
    }
}
