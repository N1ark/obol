#![feature(core_intrinsics)]

use std::any::TypeId;
use std::intrinsics;

pub fn type_id_u32() -> TypeId {
    TypeId::of::<u32>()
}

pub fn type_id_generic<T: 'static>() -> TypeId {
    TypeId::of::<T>()
}

// `type_id` is a comptime fn, so it can only be called from a const context.
pub fn type_id_intrinsic_u32() -> TypeId {
    const { intrinsics::type_id::<u32>() }
}

pub fn type_id_intrinsic_generic<T: 'static>() -> TypeId {
    const { intrinsics::type_id::<T>() }
}
