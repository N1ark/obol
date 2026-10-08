//! A method whose where-clause the concrete type doesn't satisfy leaves a vacant vtable slot; the
//! slots after it must keep rustc's indices.
trait T {
    fn needs_send(&self) -> u8
    where
        Self: Send,
    {
        0
    }
    fn dyn_callable(&self) -> u8;
}

struct S(*const ());
impl T for S {
    fn dyn_callable(&self) -> u8 {
        1
    }
}

fn main() {
    let x: &dyn T = &S(std::ptr::null());
    let _ = x.dyn_callable();
}
