//@ skip
pub union FnOrInt {
    pub f: fn() -> u32,
    pub x: usize,
}

pub static F: FnOrInt = FnOrInt { f: || 1 };
