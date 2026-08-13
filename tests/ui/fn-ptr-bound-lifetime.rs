// A function pointer's signature binds its lifetimes: `f` has type
// `for<'a> fn(Option<&'a mut u32>) -> u32`. `Option<&mut u32>` must be the same type there as
// where it is written outside the signature, so that it is declared only once.
fn get(x: Option<&mut u32>) -> u32 {
    match x {
        Some(r) => *r,
        None => 0,
    }
}

fn main() {
    let f: fn(Option<&mut u32>) -> u32 = get;
    let mut v = 1;
    let _ = f(Some(&mut v));
}
