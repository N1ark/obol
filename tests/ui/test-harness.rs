//@ rustc-args=--test --crate-type=bin
//@ obol-args=--start-from-attribute=soteria::proof
//! When compiling a test harness, the entry points are the `#[test]` functions, in source order,
//! with a `test` attribute, and the functions selected by the flags. Functions that share the name
//! of a test are not tests, nor are benchmarks.
#![feature(test, register_tool)]
#![register_tool(soteria)]
extern crate test;

pub struct Bits(pub u8);

impl Bits {
    pub fn len(&self) -> u8 {
        self.0
    }
}

impl Iterator for Bits {
    type Item = u8;
    fn next(&mut self) -> Option<u8> {
        None
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(0))
    }
}

#[test]
fn size_hint() {}

#[test]
#[should_panic]
#[ignore]
fn len() {}

#[soteria::proof]
fn harness() {}

#[bench]
fn bench(_: &mut test::Bencher) {}

mod tests {
    #[test]
    fn next() {}
}
