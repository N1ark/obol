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
    /// Build (or reuse the cached) sysroot that Obol compiles crates against: the standard library
    /// built from the toolchain's sources with Obol's MIR options.
    Setup(SetupOpts),
    /// List the targets (lib, bins, examples, tests) of a cargo project, as a
    /// JSON array of `{ "name": ..., "kind": ... }` objects. Run from within the
    /// crate directory.
    ListTargets,
    /// Print the path to the rustc toolchain used by obol.
    ToolchainPath,
    /// Print the version of the rustc toolchain used by obol.
    ToolchainVersion,
    /// Print the version of Obol (or Charon, if flag provided)
    Version(VersionOpts),
}

#[derive(Debug, Default, Clone, clap::Parser, Serialize, Deserialize)]
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
    /// The sysroot to compile the crate (and its dependencies) against: `setup` (the default) for
    /// the one built by `obol setup` (which is built if needed), `default` for the toolchain's own
    /// sysroot, or a path. For `obol rustc`, a `--sysroot` rustc argument takes precedence.
    #[clap(long = "sysroot")]
    pub sysroot: Option<String>,
    /// Args that are passed to the underlying tool (`rustc` or `cargo` depending on `--cargo`).
    #[arg(last = true)]
    pub spread: Vec<String>,
}

#[derive(Debug, Default, Clone, clap::Parser, Serialize, Deserialize)]
pub struct SetupOpts {
    /// The target to build the sysroot for (defaults to the host).
    #[clap(long = "target")]
    pub target: Option<String>,
    /// Print the path to the sysroot (and only that) on the last line of stdout.
    #[clap(long = "print-sysroot")]
    pub print_sysroot: bool,
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
