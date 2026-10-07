// Promoted constants are evaluated value globals (no initializer function returning a reference to
// a local) referenced with `&`, so they have static lifetime; inline const blocks are evaluated in
// place.
pub fn promoted() -> &'static [i32] {
    &[1, 2, 3]
}

pub fn promoted_local(x: i32) -> i32 {
    let r: &i32 = &(4 + 5);
    x + *r
}

pub fn inline_const() -> usize {
    let v = const { 3 * 7 };
    v + 1
}
