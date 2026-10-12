//@ skip
pub static ARR: [u8; 5] = *b"hello";
pub static S: &u8 = &ARR[2];
pub struct Pair {
    pub a: u32,
    pub b: u32,
}
pub static PAIR: Pair = Pair { a: 1, b: 2 };
pub static PB: &u32 = &PAIR.b;
pub static TABLE: [&u8; 2] = [&ARR[0], &ARR[4]];
