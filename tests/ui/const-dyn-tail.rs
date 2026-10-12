//! A reference constant to a struct with a `dyn Trait` tail points to a global of the concrete
//! struct type, recovered from the vtable.
trait Tr {
    fn m(&self) -> u8;
}
impl Tr for u8 {
    fn m(&self) -> u8 {
        *self
    }
}
struct W<T: ?Sized>(u8, T);

const W_DYN: &W<dyn Tr> = &W(0, 5u8);

fn tail() -> u8 {
    W_DYN.1.m()
}
