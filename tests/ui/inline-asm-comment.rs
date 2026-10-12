//! Inline assembly made only of comments, as used for optimization barriers, does nothing: an
//! `inout` operand keeps its value.
fn barrier(mut x: u64) -> u64 {
    unsafe { core::arch::asm!("/*{0}*/", inout(reg) x) };
    x
}

fn split(x: u32) -> u32 {
    let y: u32;
    unsafe { core::arch::asm!("/* {0:e} */ /* {1} */", inout(reg) x => y, in(reg) 0u64) };
    y
}
