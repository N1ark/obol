//! A generic `impl` on a tuple. Naming its monomorphized method requires translating the
//! polymorphic tuple `(A, B)`, which has no monomorphic declaration of its own: it must use the
//! generic tuple declaration of that arity instead, so that its fields stay well-scoped.
trait Fst {
    type Out;
    fn fst(self) -> Self::Out;
}

impl<A, B> Fst for (A, B) {
    type Out = A;
    fn fst(self) -> A {
        self.0
    }
}

fn main() {
    (1u8, 2u16).fst();
}
