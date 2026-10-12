//@ rustc-args=-C overflow-checks=off
// Without overflow checks, MIR's `Neg` wraps (no assert precedes it).
fn neg(x: i32) -> i32 {
    -x
}

fn main() {
    neg(1);
}
