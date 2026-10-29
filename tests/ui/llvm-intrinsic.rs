#![feature(link_llvm_intrinsics)]

unsafe extern "llvm-intrinsic" {
    #[link_name = "llvm.ctpop.i32"]
    fn ctpop(x: i32) -> i32;
}

pub fn main() {
    assert!(unsafe { ctpop(7) } == 3);
}
