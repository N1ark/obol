//@ aux-crate=closure-as-fn-in-raw-memory-aux.rs
//! A foreign static is translated from its value. Here the closure-as-fn pointer is in a union,
//! hence in raw memory; it must point to the closure-as-fn item, which takes no arguments (like
//! the `f` of `LazyLock<T, fn() -> T>`).
fn main() {
    let f = unsafe { closure_as_fn_in_raw_memory_aux::F.f };
    f();
}
