//! Constant arrays of copies of one value are translated as repeats, as in Charon, so that large
//! ones stay small.
const ZEROS: [u8; 1 << 20] = [0; 1 << 20];
const BYTES: &[u8] = &[42; 1 << 24];
const PAIR: [u16; 2] = [3, 3];
const MIXED: [u8; 3] = [1, 1, 2];
const ONE: [i64; 1] = [-1];

fn main() {
    let _ = ZEROS[5];
    let _ = BYTES[7];
    let _ = (PAIR, MIXED, ONE);
}
