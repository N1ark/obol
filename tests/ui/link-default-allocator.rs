//@ obol-args=--start-from main
//@ rustc-args=--crate-type=bin
//! With the default allocator, the allocator functions (`__rust_alloc`, ...) link to the standard
//! library's (`__rdl_alloc`, ...).
fn main() {
    unsafe { std::alloc::alloc(std::alloc::Layout::new::<u8>()) };
}
