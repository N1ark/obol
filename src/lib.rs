#![feature(rustc_private)]

pub mod args;
pub mod argsfile;
pub mod driver_env;
pub mod mir_options;
pub mod rmeta;
pub mod rustc_action;
pub mod std_crates;

/// The version of the crate, as defined in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
