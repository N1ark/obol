//! The rustc API differences between the toolchains Obol supports, confined to one place.
//!
//! The driver is written against the newest supported rustc. Older ones are selected with a
//! `cfg` set by `build.rs` from the version of the compiler building Obol (`rustc_1_98`). Besides
//! this module, version-specific code is limited to `#[cfg]`'d match arms for enum variants that
//! only exist in some versions, and to the lang-item list in `translate_from_rustc.rs`.

#[cfg(not(rustc_1_98))]
extern crate rustc_attr_ir;
#[cfg(rustc_1_98)]
extern crate rustc_hir;
extern crate rustc_public;

use rustc_public::{mir, ty};

// rustc 1.99 moved the lang items (and parsed attributes) to the new `rustc_attr_ir` crate.
#[cfg(not(rustc_1_98))]
pub(crate) use rustc_attr_ir::lang_items::LangItem;
#[cfg(rustc_1_98)]
pub(crate) use rustc_hir::lang_items::LangItem;

/// rustc_public 1.99 replaced the `span` of statements and terminators by a `source_info`.
pub(crate) fn statement_span(statement: &mir::Statement) -> &ty::Span {
    #[cfg(rustc_1_98)]
    return &statement.span;
    #[cfg(not(rustc_1_98))]
    return &statement.source_info.span;
}

/// See [`statement_span`].
pub(crate) fn terminator_span(terminator: &mir::Terminator) -> &ty::Span {
    #[cfg(rustc_1_98)]
    return &terminator.span;
    #[cfg(not(rustc_1_98))]
    return &terminator.source_info.span;
}

/// rustc 1.99 gives the `extern "unadjusted"` functions linked to LLVM intrinsics (`llvm.*`, as used
/// by `core::arch`) their own instance kind; before, they were plain foreign items.
#[cfg_attr(rustc_1_98, allow(unused_variables))]
pub(crate) fn is_llvm_intrinsic(instance: &mir::mono::Instance) -> bool {
    #[cfg(rustc_1_98)]
    return false;
    #[cfg(not(rustc_1_98))]
    return matches!(instance.kind, mir::mono::InstanceKind::LlvmIntrinsic);
}
