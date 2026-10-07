//! Tracking of non-local functions whose MIR is unavailable.
//!
//! Obol reads the bodies of dependencies (std included) from their metadata, which only contains
//! MIR for every function when the crate was compiled with Obol's MIR options
//! (`-Zalways-encode-mir` & co., see `obol_lib::mir_options`). A function without
//! MIR is translated as `Body::Missing`; this module records those per crate so we can report the
//! misconfiguration once, with the crate name, instead of silently producing an incomplete ULLBC.

use std::collections::BTreeMap;
use std::sync::Mutex;

static MISSING: Mutex<BTreeMap<String, Vec<String>>> = Mutex::new(BTreeMap::new());

/// Record that `item` (a non-local function of crate `krate`) has no MIR.
pub fn record(krate: &str, item: String) {
    MISSING
        .lock()
        .unwrap()
        .entry(krate.to_owned())
        .or_default()
        .push(item);
}

/// Print a summary of the missing bodies, as errors if `strict`. Returns whether any were missing.
pub fn report(strict: bool) -> bool {
    let missing = MISSING.lock().unwrap();
    if missing.is_empty() {
        return false;
    }
    let level = if strict { "error" } else { "warning" };
    for (krate, items) in missing.iter() {
        let examples: Vec<_> = items.iter().take(3).map(|s| format!("`{s}`")).collect();
        eprintln!(
            "{level}: no MIR available for {} function(s) of crate `{krate}` (e.g. {}); \
             `{krate}` was not compiled with Obol's MIR flags ({})",
            items.len(),
            examples.join(", "),
            obol_lib::mir_options::rustc_flags().join(" "),
        );
    }
    if strict {
        eprintln!(
            "note: under Buck2, `obol buck2` builds dependencies with \
             `-c rust.extra_rustc_flags=<MIR flags>`; a crate built by a toolchain that ignores \
             that config (or `--no-mir-flags` without an equivalent setup) lacks MIR. \
             Set {} (or pass `--allow-missing-mir`) to only warn.",
            obol_lib::driver_env::OBOL_ALLOW_MISSING_MIR
        );
    }
    true
}
