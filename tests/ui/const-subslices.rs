//! A `&str` or `&[T]` constant pointing into the middle of an allocation starts at the pointer's
//! offset, not at the allocation's start.
const STR_SUFFIX: &str = "hello".split_at(1).1;
const BYTES_SUFFIX: &[u8] = b"hello".split_at(2).1;

fn suffixes() -> (&'static str, &'static [u8]) {
    (STR_SUFFIX, BYTES_SUFFIX)
}
