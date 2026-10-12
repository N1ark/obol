//@ obol-args=--start-from crate::lines::wrap --start-from other::new
//! `--start-from` takes paths, matched against the end of the functions' paths (`crate` is the
//! local crate).
pub mod lines {
    pub fn wrap(x: u8) -> u8 {
        x
    }
    pub fn new() -> u8 {
        0
    }
}
pub mod other {
    pub fn new() -> u8 {
        1
    }
}
