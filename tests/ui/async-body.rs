//@ obol-args=--start-from-pub
//! An entry point that only builds a future, e.g. an `async fn` or an async-trait method: its
//! `poll` is translated too (for an `async fn`, the coroutine's resume function), with
//! `Waker::noop` and `Context::from_waker` to poll it with, and its drop glue.
pub async fn idx(i: usize) -> u8 {
    let v = [1u8, 2, 3];
    v[i]
}
