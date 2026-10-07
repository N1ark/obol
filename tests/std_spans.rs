//! Check that, with Obol's implicit full-MIR sysroot, the spans of standard-library items point at
//! real files of the toolchain's `rust-src` (after Obol's `/rustc/...` normalization), so that
//! Soteria can show std frames in its diagnostics.
//!
//! This builds the sysroot if it isn't cached yet (a few minutes, needs crates.io). Set
//! `OBOL_SKIP_SYSROOT_TESTS=1` to skip it.
use assert_cmd::prelude::CommandCargoExt;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn std_spans_point_at_rust_src() -> anyhow::Result<()> {
    if std::env::var_os("OBOL_SKIP_SYSROOT_TESTS").is_some() {
        eprintln!("skipped (OBOL_SKIP_SYSROOT_TESTS)");
        return Ok(());
    }
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("spans.rs");
    std::fs::write(
        &src,
        "use std::collections::HashMap;\n\
         fn main() { let mut m = HashMap::new(); m.insert(1u32, String::from(\"a\")); \
         println!(\"{}\", m[&1]); }\n",
    )?;
    let out = dir.path().join("spans.ullbc");
    let status = Command::cargo_bin("obol")?
        .args(["rustc", "--format", "json", "--dest-file"])
        .arg(&out)
        .arg("--")
        .arg(&src)
        .status()?;
    assert!(status.success(), "obol rustc failed");

    let toolchain = Command::cargo_bin("obol")?.arg("toolchain-path").output()?;
    let toolchain = PathBuf::from(String::from_utf8(toolchain.stdout)?.trim());
    let rust_src = toolchain.join("lib/rustlib/src/rust");

    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out)?)?;
    let mut checked = 0;
    for file in json["translated"]["files"].as_array().unwrap() {
        let krate = file["crate_name"].as_str().unwrap_or_default();
        if !["core", "alloc", "std"].contains(&krate) {
            continue;
        }
        let name = file["name"]
            .get("Local")
            .and_then(|n| n.as_str())
            .unwrap_or_else(|| panic!("std file without a local path: {}", file["name"]));
        let rel = name
            .strip_prefix("/rustc/")
            .unwrap_or_else(|| panic!("std file not under /rustc/: {name}"));
        let path = rust_src.join(rel);
        assert!(
            path.is_file(),
            "`{name}` doesn't exist in rust-src ({})",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 10, "only {checked} std files in the output");
    Ok(())
}
