//! A `TypeId` in the value of a constant becomes a `TypeId` constant, like in bodies.
use std::any::TypeId;

const T: TypeId = TypeId::of::<u8>();

fn is_u8(id: TypeId) -> bool {
    id == T
}
