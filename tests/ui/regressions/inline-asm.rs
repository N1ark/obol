//@ output=pretty-llbc
//! Regression test: inline `asm!` is translated to an `asm!` terminator carrying
//! its template, operands and options (it used to be an `error("Inline assembly
//! is not supported")` body). See `simple/inline-asm.rs` for all operand kinds.
use std::arch::asm;

pub unsafe fn add_via_asm(mut x: u64) -> u64 {
    asm!("add {0}, {0}", inout(reg) x);
    x
}
