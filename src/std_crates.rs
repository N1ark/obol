//! Recognizing standard-library crates without relying on the sysroot path.
//!
//! With rustup, std's sources live under `<sysroot>/lib/rustlib/src/rust/library/...` and Obol
//! normalizes them to `/rustc/library/...` by stripping the sysroot. Under Buck2 (Meta's setup),
//! std is an ordinary crate built from source and passed with `--extern`, the `--sysroot` is an
//! empty directory, and std's file names are whatever Buck's `--remap-path-prefix` made of them
//! (e.g. `third-party/rust-toolchain/1.98.1/rust/library/core/src/option.rs`). So we classify std
//! crates by crate name instead, and normalize their paths to the same `/rustc/library/...` form.

use std::path::{Component, Path, PathBuf};

/// Crates of the Rust distribution's `library/` workspace that make up the sysroot.
pub const STD_CRATES: &[&str] = &[
    "core",
    "alloc",
    "std",
    "proc_macro",
    "test",
    "compiler_builtins",
    "panic_unwind",
    "panic_abort",
    "unwind",
    "std_detect",
    "profiler_builtins",
    "rustc_std_workspace_core",
    "rustc_std_workspace_alloc",
    "rustc_std_workspace_std",
    "sysroot",
];

pub fn is_std_crate(crate_name: &str) -> bool {
    STD_CRATES.contains(&crate_name)
}

/// If `crate_name` is a std crate and `path` points into a `library/` directory, return the
/// normalized `/rustc/library/...` path, matching what Obol produces for a rustup sysroot.
pub fn normalize_std_path(crate_name: &str, path: &Path) -> Option<PathBuf> {
    if !is_std_crate(crate_name) {
        return None;
    }
    let comps: Vec<Component> = path.components().collect();
    let idx = comps
        .iter()
        .rposition(|c| matches!(c, Component::Normal(n) if *n == "library"))?;
    if idx + 1 >= comps.len() {
        return None;
    }
    // Already normalized.
    if idx == 2
        && matches!(comps[0], Component::RootDir)
        && matches!(comps[1], Component::Normal(n) if n == "rustc")
    {
        return None;
    }
    let mut out = PathBuf::from("/rustc/library");
    out.extend(comps[idx + 1..].iter());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buck_std_paths() {
        assert_eq!(
            normalize_std_path(
                "core",
                Path::new("third-party/rust-toolchain/1.98.1/rust/library/core/src/option.rs")
            ),
            Some(PathBuf::from("/rustc/library/core/src/option.rs"))
        );
        assert_eq!(
            normalize_std_path(
                "core",
                Path::new("/abs/library/stdarch/crates/core_arch/src/mod.rs")
            ),
            Some(PathBuf::from(
                "/rustc/library/stdarch/crates/core_arch/src/mod.rs"
            ))
        );
        assert_eq!(
            normalize_std_path("std", Path::new("/rustc/library/std/src/lib.rs")),
            None
        );
        // Not a std crate.
        assert_eq!(
            normalize_std_path("mylib", Path::new("library/mylib/src/lib.rs")),
            None
        );
        // No `library` component.
        assert_eq!(normalize_std_path("std", Path::new("src/lib.rs")), None);
    }
}
