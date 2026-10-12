//@ output=pretty-llbc
//! Regression test: `async fn`s and `async` blocks lower to coroutines, which we declare as
//! structs (see `coroutines.rs`).
pub async fn increment(x: u32) -> u32 {
    x + 1
}

pub fn use_async_block() -> impl std::future::Future<Output = u32> {
    async { 1u32 + 2 }
}
