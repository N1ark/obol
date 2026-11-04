//@ obol-args=--start-from main
//@ rustc-args=--crate-type=bin
//! `#[global_allocator]` defines the allocator functions (`__rust_alloc`, ...) as standard library
//! internal symbols. Those the foreign allocator functions of the program link to are translated
//! with them, with the `#[export_name]` of their symbol.
use std::alloc::{GlobalAlloc, Layout};

struct Null;

unsafe impl GlobalAlloc for Null {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}

#[global_allocator]
static A: Null = Null;

fn main() {
    unsafe { std::alloc::alloc(Layout::new::<u8>()) };
}
