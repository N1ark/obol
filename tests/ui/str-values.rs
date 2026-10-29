#![feature(adt_const_params, unsized_const_params)]
#![allow(incomplete_features)]

#[derive(std::marker::ConstParamTy, PartialEq, Eq)]
struct Wrap<T: ?Sized>(u8, T);

const WRAP: &Wrap<[u8]> = &Wrap(5, [97, 98]);
const STR_TAIL: &Wrap<str> = unsafe { &*(WRAP as *const Wrap<[u8]> as *const Wrap<str>) };

fn str_param<const S: &'static str>() -> &'static str {
    S
}

fn str_tail<const W: &'static Wrap<str>>() -> &'static Wrap<str> {
    W
}

pub fn main() {
    str_param::<"hello">();
    str_tail::<STR_TAIL>();
}
