//@ output=pretty-llbc
//! Regression test: an explicit `#[coroutine]` closure produces a coroutine type, which we
//! declare as a struct like those of `async` code.
#![feature(coroutines, coroutine_trait, stmt_expr_attributes)]

use std::ops::Coroutine;

pub fn make() -> impl Coroutine<Yield = i32, Return = ()> {
    #[coroutine]
    || {
        yield 1;
        yield 2;
    }
}
