//@ aux-crate=link-exported-aux.rs
//@ obol-args=--start-from main
//@ rustc-args=--crate-type=bin
//! A foreign function links to the function of the crate or its dependencies exported under its
//! symbol (`#[no_mangle]` or `#[export_name]`), which is translated with it, unless it is weak.
//! Unused exported functions aren't translated.
extern crate link_exported_aux;

mod sys {
    unsafe extern "Rust" {
        pub fn local_hook(x: u32) -> u32;
        pub fn dep_hook() -> u8;
    }
    unsafe extern "C" {
        pub fn weak_hook() -> u8;
    }
}

#[unsafe(no_mangle)]
fn local_hook(x: u32) -> u32 {
    x + 1
}

fn main() {
    unsafe {
        sys::local_hook(sys::dep_hook() as u32);
        sys::weak_hook();
    }
}
