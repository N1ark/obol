//! A reference constant pointing into the middle of an anonymous allocation points to a copy of
//! its pointee: the AST can't point into the middle of a global.
const P: &u8 = &b"hello"[1];
const Q: &[u8; 2] = {
    let a: &[u8; 5] = b"hello";
    unsafe { &*(a.as_ptr().add(3) as *const [u8; 2]) }
};

fn values() -> (u8, [u8; 2]) {
    (*P, *Q)
}
