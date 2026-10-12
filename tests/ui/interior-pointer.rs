//@ aux-crate=interior-pointer-aux.rs
//! Pointers into the middle of a global (here in the values of foreign statics) keep their
//! offset, as raw memory whose provenance records it.
fn values() -> (u8, u32, u8) {
    (*interior_pointer_aux::S, *interior_pointer_aux::PB, *interior_pointer_aux::TABLE[1])
}
