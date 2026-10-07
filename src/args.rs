use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, clap::ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Postcard,
    Json,
}

pub const OBOL_ARGS: &str = "OBOL_ARGS";
/// The sysroot `obol-driver` should compile target crates against, if they don't specify one.
pub const OBOL_SYSROOT: &str = "OBOL_SYSROOT";

#[derive(Debug, clap::Parser)]
#[clap(name = "Obol")]
pub enum ObolCli {
    /// Runs Obol on a single rust file (and the modules it references, if any).
    Rustc(CliOpts),
    /// Runs obol on a cargo project.
    Cargo(CliOpts),
    /// Runs obol on a Buck2 Rust target (rust_library, rust_binary or rust_test).
    ///
    /// Buck builds all dependencies, std included, with full MIR (in a dedicated isolation dir),
    /// then obol-driver re-runs the target crate's exact rustc command line and translates it.
    /// Arguments after `--` are passed to `buck2 build`/`buck2 aquery` (e.g. `@fbcode//mode/dev`).
    Buck2(Buck2Opts),
    /// List the targets (lib, bins, examples, tests) of a cargo project, as a
    /// JSON array of `{ "name": ..., "kind": ... }` objects. Run from within the
    /// crate directory.
    ListTargets,
    /// Print the path to the rustc toolchain used by obol.
    ToolchainPath,
    /// Print the sysroot `obol cargo`/`obol rustc` compile against, building Obol's full-MIR
    /// sysroot first if needed. The path is the last line of stdout. Use it to build crates that
    /// are linked into analysed code (e.g. with `--extern`) against the same standard library.
    PrintSysroot(PrintSysrootOpts),
    /// Print the version of the rustc toolchain used by obol.
    ToolchainVersion,
    /// Print the version of Obol (or Charon, if flag provided)
    Version(VersionOpts),
}

#[derive(Debug, Default, Clone, clap::Parser, Serialize, Deserialize)]
// Missing fields take their default, so that hand-written `OBOL_ARGS`/`OBOL_ARGS_FILE` JSON (e.g.
// in a Buck action) only needs the options it sets.
#[serde(default)]
pub struct CliOpts {
    /// The destination file. By default `<dest_dir>/<crate_name>.ullbc`.
    #[clap(long = "dest-file", value_parser)]
    pub dest_file: Option<PathBuf>,
    #[clap(
        long = "print-ullbc",
        help = "Print the ULLBC after applying the micro-passes."
    )]
    pub print_ullbc: bool,
    #[clap(
        long = "start-from",
        help = "List of function names that count as entry points to generate; if none are specified, main is used."
    )]
    pub start_from: Vec<String>,
    #[clap(
        long = "start-from-attribute",
        help = "List of attributes (e.g. `kani::proof`) that count as entry points to generate; empty by default."
    )]
    pub start_from_attribute: Vec<String>,
    #[clap(
        long = "start-from-pub",
        help = "Whether to start translation from all public items."
    )]
    pub start_from_pub: bool,
    #[clap(
        long = "opaque",
        help = "List of item names to keep opaque during translation"
    )]
    pub opaque: Vec<String>,
    #[clap(
        long = "no-serialize",
        help = "Whether to skip the serialization step."
    )]
    pub no_serialize: bool,
    /// Output serialization format (postcard or json).
    #[clap(long = "format", default_value = "postcard")]
    pub format: OutputFormat,
    /// Sysroot to compile the crate (and its dependencies) against. By default Obol builds (once,
    /// then caches) a sysroot whose standard library has full MIR: the production standard
    /// library compiled from the toolchain's `rust-src` with Obol's MIR options. Pass a path to
    /// use another sysroot, or `default` for the toolchain's distributed sysroot, which lacks MIR
    /// bodies for many standard library functions. (Same semantics as Charon's `--sysroot`.)
    /// For `obol rustc`, a rustc `--sysroot` argument takes precedence. Not supported by
    /// `obol buck2`, where the standard library comes from the Buck build.
    #[clap(long = "sysroot")]
    pub sysroot: Option<String>,
    /// Args that are passed to the underlying tool (`rustc`, `cargo` or `buck2`).
    #[arg(last = true)]
    pub spread: Vec<String>,
}

/// Options of `obol buck2`.
#[derive(Debug, Clone, clap::Parser)]
pub struct Buck2Opts {
    /// The Buck2 target to translate, e.g. `//foo:bar` or `fbcode//foo:bar`.
    pub target: String,
    /// Translate the crate as a test harness: adds `--test` to its rustc command line (like
    /// `cargo test --lib`) and uses the `#[test]` functions as entry points. Implied for targets
    /// whose rustc command line already has `--test` (`rust_test`).
    #[clap(long = "test")]
    pub test: bool,
    /// The `buck2` executable.
    #[clap(long = "buck2", default_value = "buck2", env = "OBOL_BUCK2")]
    pub buck2: String,
    /// Buck2 isolation dir used for the full-MIR build. Using a dedicated one keeps the
    /// `-c rust.extra_rustc_flags=...` config change from invalidating your normal daemon state.
    #[clap(long = "isolation-dir", default_value = "obol")]
    pub isolation_dir: String,
    /// Print the reconstructed obol-driver invocation (environment, arguments) and stop.
    #[clap(long = "print-command")]
    pub print_command: bool,
    /// Don't check that Buck's rustc and obol-driver are the same compiler version.
    #[clap(long = "skip-version-check")]
    pub skip_version_check: bool,
    /// Don't fail when a function from a dependency has no MIR (only warn).
    #[clap(long = "allow-missing-mir")]
    pub allow_missing_mir: bool,
    /// Don't add `-c rust.extra_rustc_flags=<Obol's MIR flags>`: use when the build already
    /// produces full-MIR metadata (e.g. through a modifier or a custom toolchain).
    #[clap(long = "no-mir-flags")]
    pub no_mir_flags: bool,
    #[clap(flatten)]
    pub obol: CliOpts,
}

/// Options of `obol print-sysroot`.
#[derive(Debug, Default, Clone, clap::Parser)]
pub struct PrintSysrootOpts {
    /// The target to get the sysroot for (defaults to the host).
    #[clap(long = "target")]
    pub target: Option<String>,
    /// Resolve this `--sysroot` value as `obol cargo`/`obol rustc` would: `default` prints the
    /// toolchain's distributed sysroot, a path is printed as is.
    #[clap(long = "sysroot")]
    pub sysroot: Option<String>,
}

#[derive(Debug, Default, Clone, clap::Parser, Serialize, Deserialize)]
pub struct VersionOpts {
    #[clap(long = "charon", help = "Print Charon version instead of Obol.")]
    pub charon: bool,
}

impl CliOpts {
    pub fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}
