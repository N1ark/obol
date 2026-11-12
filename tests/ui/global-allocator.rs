//@ obol-args=--start-from main
//@ rustc-args=--crate-type=bin
//! `#[global_allocator]` defines the allocator functions (`__rust_alloc`, ...), which the foreign
//! allocator functions of the program link to.
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
