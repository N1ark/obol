//@ obol-args=--start-from-pub
//! An entry point that is an `async fn` only builds its future: its `poll` (the coroutine's resume
//! function) is translated too, with `Waker::noop` and `Context::from_waker` to poll it with.
pub async fn idx(i: usize) -> u8 {
    let v = [1u8, 2, 3];
    v[i]
}
