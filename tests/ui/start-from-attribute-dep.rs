//@ aux-crate=start-from-attribute-dep-aux.rs
//@ obol-args=--start-from-attribute=soteria::model
//! The functions selected by `--start-from-attribute` include the monomorphic functions of the
//! dependencies with the attribute, e.g. the models of a verification tool defined in a library,
//! for its integration tests. The sysroot's crates aren't searched.
#![feature(register_tool)]
#![register_tool(soteria)]
extern crate start_from_attribute_dep_aux;

#[soteria::model]
fn local_model() -> u64 {
    7
}

fn unused() {}
