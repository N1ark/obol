//! `obol setup`: build the sysroot Obol compiles crates against.
//!
//! The standard library shipped with the toolchain is compiled with optimized MIR (and no MIR at
//! all for some items), which is not what we want to translate. Like `cargo miri setup`, we
//! instead build the standard library from the toolchain's `rust-src` with Obol's MIR options
//! (see [`obol_lib::mir_options`]). Unlike Miri, this is the production standard library: no
//! `--cfg miri`.
use anyhow::{Context, Result, bail};
use rustc_build_sysroot::{BuildMode, SysrootBuilder, SysrootConfig, SysrootStatus};
use std::{env, path::PathBuf};

use crate::toolchain;

/// Overrides the directory in which sysroots are cached.
const OBOL_SYSROOT_DIR: &str = "OBOL_SYSROOT_DIR";

/// The directory in which we cache sysroots: `$OBOL_SYSROOT_DIR`, or
/// `$XDG_CACHE_HOME/obol/sysroot` (`~/.cache/obol/sysroot` by default).
fn cache_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os(OBOL_SYSROOT_DIR) {
        return Ok(PathBuf::from(dir));
    }
    let cache = match env::var_os("XDG_CACHE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => env::home_dir()
            .context("could not determine the home directory to cache Obol's sysroot in")?
            .join(".cache"),
    };
    Ok(cache.join("obol").join("sysroot"))
}

/// Ensure the sysroot for `target` (the host by default) is built, and return its path. This is
/// fast when the sysroot is already cached. Progress messages go to stderr.
pub fn setup(target: Option<&str>) -> Result<PathBuf> {
    let rustc_path = toolchain::toolchain_path()?.join("bin").join("rustc");
    let rustc = || {
        let mut cmd = toolchain::in_toolchain(&rustc_path).unwrap();
        cmd.env_remove("RUSTC_WRAPPER");
        cmd
    };
    let version = rustc_version::VersionMeta::for_command(rustc())
        .context("failed to determine the toolchain's rustc version")?;
    let target = target.unwrap_or(&version.host).to_owned();
    let commit = version.commit_hash.clone().unwrap_or_default();

    // One sysroot per toolchain and target.
    let short_commit = &commit[..commit.len().min(12)];
    let sysroot_dir = cache_dir()?
        .join(format!("{}-{short_commit}", version.semver))
        .join(&target);

    let src_dir = rustc_build_sysroot::rustc_sysroot_src(rustc())?;
    if !src_dir.join("std").join("Cargo.toml").exists() {
        bail!(
            "could not find the standard library sources in `{}`; \
            install them with `rustup component add rust-src --toolchain {}`",
            src_dir.display(),
            toolchain::toolchain_version(),
        );
    }

    // Build with the toolchain's own cargo and rustc, ignoring the user's wrappers and flags.
    let mut cargo = toolchain::in_toolchain("cargo")?;
    cargo.env("RUSTC", &rustc_path);
    for var in [
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
    ] {
        cargo.env_remove(var);
    }

    let mut rustflags = obol_lib::mir_options::rustc_flags();
    // Make the paths to the standard library sources look like those of the distributed standard
    // library, so that they don't depend on where the toolchain is installed.
    rustflags.push(format!(
        "--remap-path-prefix={}=/rustc/{commit}/library",
        src_dir.display()
    ));

    // A full build lets the sysroot be used for anything, but needs a linker and C library for
    // the target (`std` is also built as a dylib). Obol only needs the metadata (with MIR) of the
    // standard library, which a check build provides for any target.
    let mode = if target == version.host {
        BuildMode::Build
    } else {
        BuildMode::Check
    };
    let status = SysrootBuilder::new(&sysroot_dir, &target)
        .build_mode(mode)
        .sysroot_config(SysrootConfig::WithStd {
            std_features: vec![],
        })
        .rustflags(rustflags)
        .cargo(cargo)
        .rustc_version(version)
        .when_build_required(|| {
            eprintln!(
                "Building Obol's sysroot for {target} in `{}`; this may take a few minutes...",
                sysroot_dir.display()
            )
        })
        .build_from_source(&src_dir)
        .context("failed to build Obol's sysroot")?;
    if status == SysrootStatus::SysrootBuilt {
        eprintln!("Obol's sysroot for {target} is ready.");
    }
    Ok(sysroot_dir)
}
