//! Environment variables understood by `obol-driver`, besides `OBOL_ARGS`.
//!
//! These let build systems other than cargo (Buck2, in particular) drive the driver explicitly,
//! instead of relying on the cargo heuristics (`CARGO_PRIMARY_PACKAGE`, presence of `--target`).

use std::path::PathBuf;

/// Selects how the driver treats the crate it is invoked on. See [`DriverMode`].
pub const OBOL_MODE: &str = "OBOL_MODE";
/// Output file for the translated crate. Takes precedence over `--dest-file`. Relative paths are
/// relative to the driver's working directory.
pub const OBOL_DEST: &str = "OBOL_DEST";
/// Path to a JSON file containing the serialized [`crate::args::CliOpts`]. Alternative to
/// `OBOL_ARGS` for large option sets or for build systems where passing JSON through the
/// environment is awkward.
pub const OBOL_ARGS_FILE: &str = "OBOL_ARGS_FILE";
/// When set (to anything), the driver doesn't fail when a non-local function has no MIR (it is
/// still reported as a warning). Only meaningful in [`DriverMode::Translate`].
pub const OBOL_ALLOW_MISSING_MIR: &str = "OBOL_ALLOW_MISSING_MIR";

/// How `obol-driver` treats the crate it is invoked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverMode {
    /// No `OBOL_MODE`: the historical behavior. Under cargo (`OBOL_USING_CARGO`), translate the
    /// primary packages built for `--target`; otherwise translate any crate built with `--target`
    /// and compile the others normally.
    Auto,
    /// `OBOL_MODE=translate`: always translate this crate, never emit codegen artifacts. Used when
    /// the caller knows exactly which invocation is the root (e.g. `obol buck2`).
    Translate,
    /// `OBOL_MODE=rustc`: behave like rustc (honoring the requested `--emit`), with Obol's MIR
    /// options so that the produced metadata contains full MIR. Used to build dependencies.
    Rustc,
}

impl DriverMode {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("") | Some("auto") => Ok(DriverMode::Auto),
            Some("translate") => Ok(DriverMode::Translate),
            Some("rustc") => Ok(DriverMode::Rustc),
            Some(other) => Err(format!(
                "invalid value `{other}` for {OBOL_MODE}: expected `translate`, `rustc` or `auto`"
            )),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        DriverMode::parse(std::env::var(OBOL_MODE).ok().as_deref())
    }
}

/// The output path requested through [`OBOL_DEST`], if any.
pub fn dest_from_env() -> Option<PathBuf> {
    std::env::var_os(OBOL_DEST)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modes() {
        assert_eq!(DriverMode::parse(None), Ok(DriverMode::Auto));
        assert_eq!(DriverMode::parse(Some("")), Ok(DriverMode::Auto));
        assert_eq!(
            DriverMode::parse(Some("translate")),
            Ok(DriverMode::Translate)
        );
        assert_eq!(DriverMode::parse(Some("rustc")), Ok(DriverMode::Rustc));
        assert!(DriverMode::parse(Some("bogus")).is_err());
    }
}
