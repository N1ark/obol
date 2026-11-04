//@ obol-args=--start-from-pub --opaque core::fmt --opaque core::panic
//@ rustc-args=--crate-type=lib
//! The panic handler (`#[panic_handler]`, the definition of the `panic_impl` lang item) is a
//! standard library internal symbol, which the foreign `panic_impl` that `core`'s panics call links
//! to: it is translated with it, with the `#[export_name]` of its symbol.
#![no_std]

#[panic_handler]
fn handler(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

pub fn f(x: u8) {
    panic!("{x}")
}
