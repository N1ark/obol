//! The rustc options Obol compiles every crate with (the translated crate, its dependencies and
//! the standard library of Obol's sysroot), so that we get usable MIR for all of them.

/// MIR passes we disable: they insert checks or remove information we want to model ourselves.
pub const DISABLED_MIR_PASSES: &[&str] = &[
    "RemoveStorageMarkers",
    "CheckAlignment",
    "CheckNull",
    "CheckEnums",
];

/// The rustc flags that make rustc produce MIR usable by Obol:
/// - encode the MIR of every item in the crate metadata, so that we can translate items of
///   foreign crates (including the standard library);
/// - don't optimize the MIR, and don't let MIR building exploit UB;
/// - don't run the passes in [`DISABLED_MIR_PASSES`].
pub fn rustc_flags() -> Vec<String> {
    let disabled_passes = DISABLED_MIR_PASSES
        .iter()
        .map(|pass| format!("-{pass}"))
        .collect::<Vec<_>>()
        .join(",");
    vec![
        "-Zalways-encode-mir".to_owned(),
        "-Zmir-opt-level=0".to_owned(),
        "-Zmir-preserve-ub".to_owned(),
        format!("-Zmir-enable-passes={disabled_passes}"),
    ]
}
