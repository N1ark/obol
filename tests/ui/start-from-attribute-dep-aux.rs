//@ skip
#![feature(register_tool)]
#![register_tool(soteria)]

pub fn os_seed() -> u64 {
    panic!("reads the OS")
}

#[soteria::model]
fn fixed_seed() -> u64 {
    42
}

/// Generic, so it can't be translated without an instance.
#[soteria::model]
pub fn generic_model<T>(x: T) -> T {
    x
}
