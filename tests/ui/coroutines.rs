//! Coroutines (`async fn`s and `async` blocks) are declared as structs, with one `MaybeUninit`
//! slot per local saved across an `.await`.
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

struct Pending;
impl Future for Pending {
    type Output = u32;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<u32> {
        Poll::Pending
    }
}

async fn add(x: u32) -> u32 {
    let saved = x;
    saved + Pending.await
}

fn main() {
    let mut fut = std::pin::pin!(async move { add(1).await });
    let mut cx = Context::from_waker(Waker::noop());
    let _ = fut.as_mut().poll(&mut cx);
}
