//@ output=pretty-llbc
//! Regression test: a `#[naked]` function is implemented purely with
//! `naked_asm!`; its body is a `naked_asm!` terminator (it used to be an
//! `error("Inline assembly is not supported")` body).
#![feature(naked_functions)]

use std::arch::naked_asm;

#[unsafe(naked)]
pub extern "C" fn just_return() {
    naked_asm!("ret");
}
