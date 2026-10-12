//! `core::arch` functions call LLVM intrinsics, declared as `extern "unadjusted"` functions linked
//! to `llvm.*`: they are translated as extern functions.
use std::arch::x86_64::{_MM_HINT_T0, _mm_prefetch};

fn prefetch(p: *const i8) {
    unsafe { _mm_prefetch::<_MM_HINT_T0>(p) }
}
