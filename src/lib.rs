#![feature(rustc_private)]

pub mod args;
pub mod mir_options;

/// The version of the crate, as defined in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
