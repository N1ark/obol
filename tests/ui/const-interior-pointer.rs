//! A reference constant pointing into the middle of an anonymous allocation keeps its offset, as
//! raw memory whose provenance records it.
const P: &u8 = &b"hello"[1];
const Q: &[u8; 2] = {
    let a: &[u8; 5] = b"hello";
    unsafe { &*(a.as_ptr().add(3) as *const [u8; 2]) }
};

fn values() -> (u8, [u8; 2]) {
    (*P, *Q)
}
