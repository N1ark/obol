//! The sysroot Obol compiles crates against (for `obol cargo` and `obol rustc`, unless
//! `--sysroot` says otherwise).
//!
//! The standard library shipped with the toolchain is compiled with optimized MIR (and no MIR at
//! all for some items), which is not what we want to translate. Like `cargo miri setup`, we
//! instead build the standard library from the toolchain's `rust-src` with Obol's MIR options
//! (see [`obol_lib::mir_options`]). Unlike Miri, this is the production standard library: no
//! `--cfg miri`.
//!
//! The sysroot is built on first use and cached per toolchain, MIR options and target. Concurrent
//! Obol runs serialize the build with a lock file, and a cached sysroot that is incomplete or
//! corrupt (e.g. files deleted, or left behind by an interrupted build) is detected and rebuilt.
use anyhow::{Context, Result, bail};
use rustc_build_sysroot::{BuildMode, SysrootBuilder, SysrootConfig, SysrootStatus};
use std::{env, path::PathBuf, process::Command};

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

/// A short, stable (FNV-1a) hash of the flags the sysroot is built with. It is part of the cache
/// path: `rustc-build-sysroot` would otherwise rebuild a sysroot in place when the flags change,
/// and cargo doesn't notice that the crates it already compiled against it are now stale.
fn flags_hash(flags: &[String]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in flags.iter().flat_map(|flag| flag.bytes().chain([0])) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", hash >> 32)
}

/// Ensure the sysroot for `target` (the host by default) is built, and return its path. This is
/// fast when the sysroot is already cached. Progress messages go to stderr.
pub fn ensure_sysroot(target: Option<&str>) -> Result<PathBuf> {
    let rustc_path = toolchain::toolchain_path()?.join("bin").join("rustc");
    let rustc = || -> Result<Command> {
        let mut cmd = toolchain::in_toolchain(&rustc_path)?;
        cmd.env_remove("RUSTC_WRAPPER");
        Ok(cmd)
    };
    let version = rustc_version::VersionMeta::for_command(rustc()?)
        .context("failed to determine the toolchain's rustc version")?;
    let target = target.unwrap_or(&version.host).to_owned();
    let expected_version = version.short_version_string.clone();
    let commit = version.commit_hash.clone().unwrap_or_default();
    let rustflags = obol_lib::mir_options::rustc_flags();

    // One sysroot per toolchain, set of MIR options and target.
    let short_commit = &commit[..commit.len().min(12)];
    let sysroot_dir = cache_dir()?
        .join(format!(
            "{}-{short_commit}-{}",
            version.semver,
            flags_hash(&rustflags)
        ))
        .join(&target);

    // Serialize concurrent builds of the same sysroot (e.g. several `obol cargo` runs started in
    // parallel on a fresh machine). `rustc-build-sysroot` installs atomically, but without the
    // lock each run would build its own copy. The lock is released when `lock` is dropped, at the
    // end of this function.
    std::fs::create_dir_all(&sysroot_dir)
        .with_context(|| format!("failed to create `{}`", sysroot_dir.display()))?;
    let lock_path = sysroot_dir.join(".obol-sysroot.lock");
    let lock = std::fs::File::create(&lock_path)
        .with_context(|| format!("failed to create `{}`", lock_path.display()))?;
    if lock.try_lock().is_err() {
        eprintln!(
            "Waiting for another Obol process to finish building the sysroot in `{}`...",
            sysroot_dir.display()
        );
        lock.lock()
            .with_context(|| format!("failed to lock `{}`", lock_path.display()))?;
    }

    // `rustc-build-sysroot` only rebuilds when its hash file is missing or outdated. Also rebuild
    // when the libraries themselves are missing or unreadable.
    let target_dir = sysroot_dir.join("lib").join("rustlib").join(&target);
    if target_dir.exists()
        && let Err(problem) = check_sysroot(&target_dir, &expected_version)
    {
        eprintln!(
            "Obol's cached sysroot in `{}` is unusable ({problem}); rebuilding it.",
            sysroot_dir.display()
        );
        std::fs::remove_dir_all(&target_dir)
            .with_context(|| format!("failed to remove `{}`", target_dir.display()))?;
    }

    let src_dir = rustc_build_sysroot::rustc_sysroot_src(rustc()?)?;
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

    // No `--remap-path-prefix` (like cargo-miri): the standard library's spans must stay real
    // paths into the toolchain's `rust-src`. rustc only maps `/rustc/<hash>/...` back to local
    // files when the sysroot itself contains `lib/rustlib/src/rust`, which ours doesn't, so a remap
    // would leave nonexistent paths in the output.

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
        check_sysroot(&target_dir, &expected_version).map_err(|problem| {
            anyhow::anyhow!("the freshly built sysroot is unusable: {problem}")
        })?;
        eprintln!("Obol's sysroot for {target} is ready.");
    }
    drop(lock);
    Ok(sysroot_dir)
}

/// Check that a cached sysroot (`<sysroot>/lib/rustlib/<target>`) contains the metadata (`.rmeta`,
/// or `.rlib` for crates built with codegen, like `std` in a full build) of the
/// standard library crates, readable and produced by the expected compiler (`rustc --version`).
fn check_sysroot(target_dir: &std::path::Path, rustc_version: &str) -> Result<(), String> {
    let lib = target_dir.join("lib");
    let entries =
        std::fs::read_dir(&lib).map_err(|e| format!("cannot read `{}`: {e}", lib.display()))?;
    let files: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    for krate in ["core", "alloc", "std"] {
        let prefix = format!("lib{krate}-");
        let rmeta = files
            .iter()
            .find(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n.starts_with(&prefix) && (n.ends_with(".rmeta") || n.ends_with(".rlib"))
                })
            })
            .ok_or_else(|| format!("no metadata for `{krate}`"))?;
        match obol_lib::rmeta::version_from_rmeta_file(rmeta) {
            Ok(Some(v)) if v == rustc_version => {}
            Ok(Some(v)) => return Err(format!("`{}` was built by `{v}`", rmeta.display())),
            Ok(None) | Err(_) => {
                return Err(format!("`{}` is not valid metadata", rmeta.display()));
            }
        }
    }
    Ok(())
}
