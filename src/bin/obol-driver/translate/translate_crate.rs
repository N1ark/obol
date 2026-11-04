extern crate rustc_abi;
extern crate rustc_ast;
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_public;
extern crate rustc_public_bridge;
extern crate rustc_span;
extern crate rustc_structures;
extern crate rustc_target;

use crate::translate::my_gen_args::MyGenericArgs;
use obol_lib::args::CliOpts;

use super::translate_ctx::*;

use charon_lib::ast::*;
use charon_lib::errors::ErrorCtx;
use charon_lib::options::{CliOpts as CharonCliOpts, TranslateOptions};
use charon_lib::transform::TransformCtx;
use itertools::Itertools;
use log::trace;
use rustc_middle::ty::TyCtxt;
use rustc_public::mir::mono::Instance;
use rustc_public::rustc_internal::{self};
use rustc_public::{CrateDef, DefId, mir, ty};
use rustc_public_bridge::IndexedVal;
use rustc_span::{Symbol, sym};
use std::cell::RefCell;
use std::fmt::Debug;
use std::hash::Hash;
use std::path::PathBuf;

/// The type recorded alongside a `Global`'s allocation. It is only used to translate the
/// allocation's bytes; it is deliberately *not* part of the global's identity. A given allocation
/// is a single object, so every pointer into it must resolve to the same global no matter which
/// pointee type a particular pointer views it through (e.g. `&Aligned` vs a `*const u8` into it) —
/// otherwise pointer identity/sharing across constants is lost. Globals are therefore identified by
/// their allocation id alone, and the first registration's type wins.
#[derive(Clone, Debug)]
pub struct GlobalTy(pub Option<ty::Ty>);

impl PartialEq for GlobalTy {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
impl Eq for GlobalTy {}
impl std::hash::Hash for GlobalTy {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// A tuple type, as identified by its (already translated) element types. We deliberately key on
/// those rather than on the rustc type: we erase lifetimes, so two rustc tuples that differ only
/// in their lifetimes are one and the same type to us, and must share a single declaration. The
/// `ty::Ty` is only kept to compute the layout, so it is not part of the identity and the first
/// registration's wins.
#[derive(Clone, Debug)]
pub struct TupleTy {
    pub fields: Vec<Ty>,
    pub rty: ty::Ty,
}

impl TupleTy {
    /// The generic element types of the generic tuple declaration of the given arity.
    pub fn generic_fields(arity: usize) -> Vec<Ty> {
        (0..arity)
            .map(|i| TyKind::TypeVar(TypeDbVar::Free(TypeVarId::from_usize(i))).into_ty())
            .collect()
    }

    /// Whether this is the generic tuple declaration of its arity, rather than an instantiation.
    pub fn is_generic(&self) -> bool {
        !self.fields.is_empty() && self.fields == Self::generic_fields(self.fields.len())
    }
}

impl PartialEq for TupleTy {
    fn eq(&self, other: &Self) -> bool {
        self.fields == other.fields
    }
}
impl Eq for TupleTy {}
impl Hash for TupleTy {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.fields.hash(state);
    }
}

/// The id of an untranslated item. Note that a given `DefId` may show up as multiple different
/// item sources, e.g. a constant will have both a `Global` version (for the constant itself) and a
/// `FunDecl` one (for its initializer function).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransItemSource {
    Global(mir::alloc::AllocId, GlobalTy), // the static or const itself, with its type
    NamedConst(ty::ConstDef, MyGenericArgs), // a named (top-level or associated) const item
    Fun(mir::mono::Instance),
    Type(ty::AdtDef, MyGenericArgs),
    Closure(ty::ClosureDef, MyGenericArgs),
    ClosureAsFn(ty::ClosureDef, MyGenericArgs),
    ForeignType(ty::ForeignDef),
    /// A tuple type. Tuples have no `DefId`, but like `str` they get a type declaration of their
    /// own, which we declare as a struct of its elements.
    Tuple(TupleTy),
    /// The `str` type, which we declare as a `struct str([u8])`.
    Str,
    VTable(ty::Ty, Option<(ty::TraitDef, MyGenericArgs)>),
    TraitDecl(DefId),
    TraitImpl(DefId),
}

impl TransItemSource {
    pub(crate) fn as_def_id(&self) -> Option<DefId> {
        match self {
            TransItemSource::Global(id, _) => {
                let glob_alloc: mir::alloc::GlobalAlloc = id.clone().into();
                match glob_alloc {
                    mir::alloc::GlobalAlloc::Function(instance) => Some(instance.def.def_id()),
                    mir::alloc::GlobalAlloc::Static(static_def) => Some(static_def.def_id()),
                    _ => None,
                }
            }
            TransItemSource::Fun(instance) => Some(instance.def.def_id()),
            TransItemSource::NamedConst(def, _) => Some(def.0),
            TransItemSource::Type(id, _) => Some(id.0),
            TransItemSource::Closure(def, _) => Some(def.def_id()),
            TransItemSource::ClosureAsFn(def, _) => Some(def.def_id()),
            TransItemSource::ForeignType(def) => Some(def.def_id()),
            TransItemSource::Tuple(..) | TransItemSource::Str => None,
            TransItemSource::VTable(_, Some((tr, _))) => Some(tr.0),
            TransItemSource::VTable(_, None) => None,
            TransItemSource::TraitDecl(did) => Some(*did),
            TransItemSource::TraitImpl(did) => Some(*did),
        }
    }

    /// Value with which we order values. This key may collide for distinct (under `Eq`) sources, so
    /// it must not be the sole key of a deduplicating container (see `items_to_translate`).
    pub(crate) fn sort_key(&self) -> (usize, usize, usize) {
        fn key_instance(k: &mir::mono::InstanceKind) -> usize {
            match k {
                mir::mono::InstanceKind::Intrinsic => 0,
                mir::mono::InstanceKind::Item => 1,
                mir::mono::InstanceKind::Shim => 2,
                mir::mono::InstanceKind::LlvmIntrinsic => 3,
                mir::mono::InstanceKind::Virtual { idx } => 4 + *idx,
            }
        }
        fn key_trait(t: &Option<(ty::TraitDef, MyGenericArgs)>) -> usize {
            match t {
                None => 0,
                Some((tr, args)) => tr.0.to_index() * 31 + args.sort_key(),
            }
        }

        match self {
            TransItemSource::Global(id, _) => (0, id.to_index(), 0),
            TransItemSource::Fun(instance) => {
                (1, instance.def.to_index(), key_instance(&instance.kind))
            }
            TransItemSource::Type(id, gargs) => (2, id.0.to_index(), gargs.sort_key()),
            TransItemSource::Closure(def, gargs) => (3, def.def_id().to_index(), gargs.sort_key()),
            TransItemSource::ClosureAsFn(def, gargs) => {
                (4, def.def_id().to_index(), gargs.sort_key())
            }
            TransItemSource::ForeignType(def) => (5, def.def_id().to_index(), 0),
            TransItemSource::VTable(ty, t) => (6, ty.to_index(), key_trait(t)),
            TransItemSource::NamedConst(def, gargs) => (11, def.0.to_index(), gargs.sort_key()),
            TransItemSource::TraitDecl(did) => (12, did.to_index(), 0),
            TransItemSource::TraitImpl(did) => (13, did.to_index(), 0),
            TransItemSource::Tuple(tuple) => (14, tuple.fields.len(), 0),
            TransItemSource::Str => (15, 0, 0),
        }
    }
}

/// Manual impls because `DefId` is not orderable.
impl PartialOrd for TransItemSource {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for TransItemSource {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.sort_key().cmp(&other.sort_key())
    }
}

impl<'tcx, 'ctx> TranslateCtx<'tcx> {
    pub(crate) fn register_id_no_enqueue(
        &mut self,
        src: &Option<DepSource>,
        id: TransItemSource,
    ) -> ItemId {
        let item_id = match self.id_map.get(&id) {
            Some(tid) => *tid,
            None => {
                let trans_id = match id {
                    TransItemSource::Type(..)
                    | TransItemSource::Closure(..)
                    | TransItemSource::ForeignType(..)
                    | TransItemSource::Tuple(..)
                    | TransItemSource::Str => {
                        ItemId::Type(self.translated.type_decls.reserve_slot())
                    }
                    TransItemSource::Global(..)
                    | TransItemSource::VTable(..)
                    | TransItemSource::NamedConst(..) => {
                        ItemId::Global(self.translated.global_decls.reserve_slot())
                    }
                    TransItemSource::Fun(..) | TransItemSource::ClosureAsFn(..) => {
                        ItemId::Fun(self.translated.fun_decls.reserve_slot())
                    }
                    TransItemSource::TraitDecl(..) => {
                        ItemId::TraitDecl(self.translated.trait_decls.reserve_slot())
                    }
                    TransItemSource::TraitImpl(..) => {
                        ItemId::TraitImpl(self.translated.trait_impls.reserve_slot())
                    }
                };
                // Add the id to the queue of declarations to translate
                self.id_map.insert(id.clone(), trans_id);
                self.reverse_id_map.insert(trans_id, id.clone());
                trans_id
            }
        };
        self.errors
            .borrow_mut()
            .register_dep_source(src, item_id, true);
        item_id
    }

    /// Register this id and enqueue it for translation.
    pub(crate) fn register_and_enqueue_id(
        &mut self,
        src: &Option<DepSource>,
        id: TransItemSource,
    ) -> ItemId {
        // The queue is keyed by `(sort_key, item_id)`, not the source: `sort_key` collides for some
        // distinct sources, and a `BTreeSet<TransItemSource>` would silently drop one (reserving its
        // id but never translating it). We don't enqueue polymorphic items: their bodies can't be
        // translated monomorphically (layout/normalization failures, even rustc ICEs).
        let enqueue = !self.source_is_polymorphic(&id);
        let item_id = self.register_id_no_enqueue(src, id.clone());
        if enqueue {
            self.items_to_translate.insert((id.sort_key(), item_id));
        }
        item_id
    }

    /// Whether a query's generic arguments still contain free type/const parameters, i.e. the item
    /// is not fully monomorphized and so can't have its body translated.
    fn source_is_polymorphic(&self, id: &TransItemSource) -> bool {
        let gargs = match id {
            TransItemSource::Type(_, g)
            | TransItemSource::Closure(_, g)
            | TransItemSource::ClosureAsFn(_, g)
            | TransItemSource::NamedConst(_, g) => g,
            _ => return false,
        };
        self.generic_args_have_params(&gargs.clone().into())
    }

    /// Whether this type (recursively) mentions any free type or const parameter.
    pub(crate) fn ty_has_params(&self, ty: ty::Ty) -> bool {
        use rustc_middle::ty::TypeVisitableExt;
        rustc_internal::internal(self.tcx, ty).has_param()
    }

    /// Whether these generic arguments (recursively) mention any free type or const parameter.
    pub(crate) fn generic_args_have_params(&self, gargs: &ty::GenericArgs) -> bool {
        use rustc_middle::ty::TypeVisitableExt;
        gargs.0.iter().any(|kind| match kind {
            ty::GenericArgKind::Type(t) => rustc_internal::internal(self.tcx, *t).has_param(),
            ty::GenericArgKind::Const(c) => rustc_internal::internal(self.tcx, c).has_param(),
            ty::GenericArgKind::Lifetime(_) => false,
        })
    }

    pub(crate) fn register_type_decl_id(
        &mut self,
        src: &Option<DepSource>,
        id: ty::AdtDef,
        genargs: ty::GenericArgs,
    ) -> TypeDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::Type(id, genargs.into()))
            .as_type()
            .unwrap()
    }

    /// Register the type declaration of a tuple type; `fields` are its already-translated element
    /// types.
    pub(crate) fn register_tuple_decl_id(
        &mut self,
        src: &Option<DepSource>,
        rty: ty::Ty,
        fields: Vec<Ty>,
    ) -> TypeDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::Tuple(TupleTy { fields, rty }))
            .as_type()
            .unwrap()
    }

    /// The element types of the tuple declared by `id`, if it declares a tuple at all.
    pub(crate) fn tuple_fields(&self, id: TypeDeclId) -> Option<&[Ty]> {
        match self.reverse_id_map.get(&ItemId::Type(id))? {
            TransItemSource::Tuple(tuple) => Some(&tuple.fields),
            _ => None,
        }
    }

    pub(crate) fn register_str_decl_id(&mut self, src: &Option<DepSource>) -> TypeDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::Str)
            .as_type()
            .unwrap()
    }

    /// Claim the first type id for the declaration of the unit type: charon's `Ty::mk_unit`
    /// hardcodes `TypeDeclId::UNIT`, so that id must be the unit type's declaration.
    pub(crate) fn reserve_unit_decl(&mut self) {
        let id = self.register_tuple_decl_id(&None, ty::Ty::new_tuple(&[]), vec![]);
        assert_eq!(id, TypeDeclId::UNIT, "the unit type must come first");
    }

    pub(crate) fn register_fun_decl_id(
        &mut self,
        src: &Option<DepSource>,
        id: mir::mono::Instance,
    ) -> FunDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::Fun(id))
            .as_fun()
            .unwrap()
    }

    pub(crate) fn register_closure_type_decl_id(
        &mut self,
        src: &Option<DepSource>,
        closure: ty::ClosureDef,
        args: ty::GenericArgs,
    ) -> TypeDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::Closure(closure, args.into()))
            .as_type()
            .unwrap()
    }

    pub(crate) fn register_closure_as_fn_id(
        &mut self,
        src: &Option<DepSource>,
        closure: ty::ClosureDef,
        args: ty::GenericArgs,
    ) -> FunDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::ClosureAsFn(closure, args.into()))
            .as_fun()
            .unwrap()
    }

    pub(crate) fn register_global_decl_id(
        &mut self,
        src: &Option<DepSource>,
        stt: mir::alloc::AllocId,
        ty: Option<ty::Ty>,
    ) -> GlobalDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::Global(stt, GlobalTy(ty)))
            .as_global()
            .unwrap()
    }

    pub(crate) fn register_foreign_type_decl_id(
        &mut self,
        src: &Option<DepSource>,
        def: ty::ForeignDef,
    ) -> TypeDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::ForeignType(def))
            .as_type()
            .unwrap()
    }

    pub(crate) fn register_vtable(
        &mut self,
        src: &Option<DepSource>,
        ty: ty::Ty,
        traitdef: Option<(ty::TraitDef, MyGenericArgs)>,
    ) -> GlobalDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::VTable(ty, traitdef))
            .as_global()
            .unwrap()
    }

    pub(crate) fn register_global_from_static(
        &mut self,
        src: &Option<DepSource>,
        stt: mir::mono::StaticDef,
    ) -> GlobalDeclId {
        // Go through the static's allocation, so that it shares its declaration with the pointers
        // to it (see `translate_alloc_target`).
        let def_id = rustc_internal::internal(self.tcx, stt.def_id());
        let alloc_id = self.tcx.reserve_and_set_static_alloc(def_id);
        self.register_global_decl_id(src, rustc_internal::stable(alloc_id), Some(stt.ty()))
    }

    pub(crate) fn register_named_const(
        &mut self,
        src: &Option<DepSource>,
        def: ty::ConstDef,
        args: MyGenericArgs,
    ) -> GlobalDeclId {
        *self
            .register_and_enqueue_id(src, TransItemSource::NamedConst(def, args))
            .as_global()
            .unwrap()
    }
}

// Id and item reference registration.
impl<'tcx, 'ctx> ItemTransCtx<'tcx, 'ctx> {
    pub(crate) fn make_dep_source(&self, span: Span) -> Option<DepSource> {
        Some(DepSource {
            src_id: self.item_id?,
            span: Some(span),
        })
    }

    pub(crate) fn register_type_decl_id(
        &mut self,
        span: Span,
        id: ty::AdtDef,
        genargs: ty::GenericArgs,
    ) -> TypeDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx.register_type_decl_id(&src, id, genargs)
    }

    pub(crate) fn register_tuple_decl_id(
        &mut self,
        span: Span,
        ty: ty::Ty,
        fields: Vec<Ty>,
    ) -> TypeDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx.register_tuple_decl_id(&src, ty, fields)
    }

    pub(crate) fn register_str_decl_id(&mut self, span: Span) -> TypeDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx.register_str_decl_id(&src)
    }

    pub(crate) fn register_fun_decl_id(
        &mut self,
        span: Span,
        id: mir::mono::Instance,
    ) -> FunDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx.register_fun_decl_id(&src, id)
    }

    pub(crate) fn register_closure_type_decl_id(
        &mut self,
        span: Span,
        closure: ty::ClosureDef,
        args: ty::GenericArgs,
    ) -> TypeDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx
            .register_closure_type_decl_id(&src, closure, args)
    }

    pub(crate) fn register_closure_as_fn_id(
        &mut self,
        span: Span,
        closure: ty::ClosureDef,
        args: ty::GenericArgs,
    ) -> FunDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx.register_closure_as_fn_id(&src, closure, args)
    }

    pub(crate) fn register_global_decl_id(
        &mut self,
        span: Span,
        stt: mir::alloc::AllocId,
        ty: Option<ty::Ty>,
    ) -> GlobalDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx.register_global_decl_id(&src, stt, ty)
    }

    pub(crate) fn register_foreign_type_decl_id(
        &mut self,
        span: Span,
        def: ty::ForeignDef,
    ) -> TypeDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx.register_foreign_type_decl_id(&src, def)
    }

    pub(crate) fn register_vtable(
        &mut self,
        span: Span,
        ty: ty::Ty,
        traitdef: Option<ty::TraitRef>,
    ) -> GlobalDeclId {
        let src = self.make_dep_source(span);
        let traitdef = traitdef.map(|t| (t.def_id, t.args().clone().into()));
        self.t_ctx.register_vtable(&src, ty, traitdef)
    }

    pub(crate) fn register_global_from_static(
        &mut self,
        span: Span,
        stt: mir::mono::StaticDef,
    ) -> GlobalDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx.register_global_from_static(&src, stt)
    }

    pub(crate) fn register_named_const(
        &mut self,
        span: Span,
        def: ty::ConstDef,
        args: MyGenericArgs,
    ) -> GlobalDeclId {
        let src = self.make_dep_source(span);
        self.t_ctx.register_named_const(&src, def, args)
    }

    pub(crate) fn register_trait_decl_id(
        &mut self,
        span: Span,
        trait_def: ty::TraitDef,
    ) -> TraitDeclId {
        let src = self.make_dep_source(span);
        *self
            .t_ctx
            .register_and_enqueue_id(&src, TransItemSource::TraitDecl(trait_def.0))
            .as_trait_decl()
            .unwrap()
    }
}

impl<'tcx> TranslateCtx<'tcx> {
    /// Enqueues the definitions the foreign function `symbol` links to (see `exported_fns`). They
    /// are roots of the translation, like entry points: nothing refers to them but their symbol.
    pub(crate) fn enqueue_linked_definitions(&mut self, symbol: &str) {
        let tcx = self.tcx;
        let exported = self
            .exported_fns
            .get_or_insert_with(|| exported_fns(tcx))
            .get(symbol)
            .cloned()
            .unwrap_or_default();
        for def_id in exported {
            let instance = rustc_middle::ty::Instance::mono(tcx, def_id);
            let id = self.register_fun_decl_id(&None, rustc_internal::stable(instance));
            self.linked_definitions.insert(ItemId::Fun(id));
        }
    }

    /// Resolve `core::intrinsics::$name` to its `DefId`.
    ///
    /// Rustc exposes no name -> `DefId` map for intrinsics, so we walk down `core`'s module tree
    /// to find it. Returns `None` if the name doesn't resolve to an intrinsic of that name.
    pub(crate) fn resolve_intrinsic(&self, name: Symbol) -> Option<rustc_span::def_id::DefId> {
        let core = *self
            .tcx
            .crates(())
            .iter()
            .find(|&&krate| self.tcx.crate_name(krate) == sym::core)?;
        let child = |def_id, name| {
            Some(
                self.tcx
                    .module_children(def_id)
                    .iter()
                    .find(|child| child.ident.name == name)?
                    .res
                    .opt_def_id()?,
            )
        };
        let def_id = child(child(core.as_def_id(), sym::intrinsics)?, name)?;
        self.tcx.is_intrinsic(def_id, name).then_some(def_id)
    }

    /// The `#[test]` functions of the crate, when it is compiled as a test harness (`rustc --test`).
    ///
    /// The harness expansion consumes the `#[test]` attribute of a test function: it leaves the
    /// function unmarked, next to a generated `#[rustc_test_marker]` item (a `test::TestDescAndFn`)
    /// whose initializer calls it. We find each test function as the argument-less local function
    /// its marker calls (benchmarks take a `Bencher`). Matching by name instead would also pick up
    /// unrelated functions that share a test's name, e.g. trait methods.
    fn collect_test_functions(&self) -> std::collections::HashSet<rustc_hir::def_id::DefId> {
        use rustc_hir::def::{DefKind, Res};
        use rustc_hir::intravisit::{self, Visitor};
        use rustc_hir::{Expr, ExprKind, QPath};

        /// Collects the local functions an expression refers to, including in closures.
        struct LocalFns<'tcx> {
            tcx: TyCtxt<'tcx>,
            fns: Vec<rustc_hir::def_id::DefId>,
        }
        impl<'tcx> Visitor<'tcx> for LocalFns<'tcx> {
            type NestedFilter = rustc_middle::hir::nested_filter::OnlyBodies;
            fn maybe_tcx(&mut self) -> TyCtxt<'tcx> {
                self.tcx
            }
            fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
                if let ExprKind::Path(QPath::Resolved(None, path)) = expr.kind
                    && let Res::Def(DefKind::Fn, def_id) = path.res
                    && def_id.is_local()
                {
                    self.fns.push(def_id);
                }
                intravisit::walk_expr(self, expr)
            }
        }

        let tcx = self.tcx;
        if !tcx.sess.is_test_crate() {
            return Default::default();
        }
        tcx.hir_crate_items(())
            .free_items()
            .map(|item| item.owner_id.def_id)
            .filter(|&marker| {
                let attrs = tcx.hir_attrs(tcx.local_def_id_to_hir_id(marker));
                rustc_hir::find_attr!(attrs, RustcTestMarker(..))
            })
            .flat_map(|marker| {
                let mut visitor = LocalFns {
                    tcx,
                    fns: Vec::new(),
                };
                visitor.visit_body(tcx.hir_body_owned_by(marker));
                visitor.fns
            })
            .filter(|&f| {
                tcx.fn_sig(f)
                    .skip_binder()
                    .inputs()
                    .skip_binder()
                    .is_empty()
            })
            .collect()
    }

    fn collect_entrypoints(&mut self, options: &CliOpts) -> Vec<ItemId> {
        // When compiling a test binary, the `#[test]` functions are entry points. If there are any,
        // the selection by flags only adds to them (selecting everything would otherwise pick up
        // the generated `main`).
        let has_tests = !self.test_functions.is_empty();

        let collect_all = !has_tests
            && options.start_from.is_empty()
            && options.start_from_attribute.is_empty()
            && !options.start_from_pub;

        rustc_public::all_local_items()
            .iter()
            .filter_map(|item| {
                let Ok(instance) = Instance::try_from(*item) else {
                    return None;
                };

                let def_id = rustc_internal::internal(self.tcx, instance.def.def_id());

                if matches!(
                    self.tcx.def_kind(def_id),
                    rustc_hir::def::DefKind::GlobalAsm
                ) {
                    return None;
                }
                // Only collect monomorphic items.
                if !matches!(item.kind(), rustc_public::ItemKind::Fn) {
                    return None;
                }

                if self.test_functions.contains(&def_id) {
                    return Some(instance);
                }

                if collect_all {
                    return Some(instance);
                }

                if options.start_from_pub && self.tcx.visibility(def_id).is_public() {
                    return Some(instance);
                }

                let instance_name = instance.name();
                let name_split = instance_name.split("::").last().unwrap();
                if options.start_from.contains(&name_split.to_string()) {
                    return Some(instance);
                }

                if self.has_start_attribute(options, def_id) {
                    return Some(instance);
                }

                None
            })
            .sorted_by_key(|i| i.def.def_id().to_index())
            .chain(self.dependency_items_with_start_attribute(options))
            .map(|instance| ItemId::Fun(self.register_fun_decl_id(&None, instance)))
            .collect()
    }

    /// Whether the item has one of the attributes of `--start-from-attribute`.
    fn has_start_attribute(&self, options: &CliOpts, def_id: rustc_hir::def_id::DefId) -> bool {
        self.attributes_for(def_id).iter().any(|a| match a {
            rustc_hir::Attribute::Parsed(..) => false,
            rustc_hir::Attribute::Unparsed(attr) => {
                let path = attr.path.segments.iter().map(|i| i.to_string()).join("::");
                options.start_from_attribute.contains(&path)
            }
        })
    }

    /// The monomorphic functions of the dependencies that have one of the attributes of
    /// `--start-from-attribute`, by crate then in definition order. Attributes select items in the
    /// whole crate graph, e.g. so that the models a library defines for a verification tool apply
    /// to its integration tests, which are crates of their own. The crates of the sysroot are
    /// skipped: they are not compiled with the user's attributes, and are much larger.
    fn dependency_items_with_start_attribute(&self, options: &CliOpts) -> Vec<Instance> {
        if options.start_from_attribute.is_empty() {
            return Vec::new();
        }
        let tcx = self.tcx;
        let sysroot_lib = &tcx.sess.target_tlib_path.dir;
        let mut items: Vec<(rustc_hir::def_id::DefId, Instance)> = rustc_public::external_crates()
            .into_iter()
            .filter(|krate| {
                let cnum = rustc_internal::internal(tcx, krate.id);
                !tcx.crate_dep_kind(cnum).macros_only()
                    && !tcx
                        .used_crate_source(cnum)
                        .paths()
                        .any(|path| path.starts_with(sysroot_lib))
            })
            .flat_map(|krate| krate.fn_defs())
            .filter_map(|fun| {
                let def_id = rustc_internal::internal(tcx, fun.def_id());
                if !self.has_start_attribute(options, def_id) {
                    return None;
                }
                let instance = Instance::try_from(rustc_public::CrateItem(fun.def_id())).ok()?;
                Some((def_id, instance))
            })
            .collect();
        items.sort_by_key(|(def_id, _)| (def_id.krate, def_id.index));
        items.into_iter().map(|(_, instance)| instance).collect()
    }

    /// Constructors that run before the program starts; which ones is decided by the linker,
    /// which is a bit obscure, so we follow the same logic as Miri and charon. See:
    /// https://github.com/rust-lang/miri/blob/ddd2b2fc87565b16fd38e7cab9838295aaca1c40/src/shims/global_ctor.rs
    fn collect_global_ctors(&mut self) -> Vec<GlobalDeclId> {
        use rustc_middle::middle::codegen_fn_attrs::CodegenFnAttrFlags;
        use rustc_middle::middle::exported_symbols::ExportedSymbol;
        use rustc_span::def_id::DefId as RustcDefId;
        use rustc_structures::CrateType;
        use rustc_target::spec::BinaryFormat;

        let tcx = self.tcx;
        let is_ctor_section = |section: &str| match tcx.sess.target.binary_format {
            BinaryFormat::Coff => section == ".CRT$XCU",
            BinaryFormat::MachO => {
                let mut parts = section.splitn(3, ',');
                parts.next() == Some("__DATA")
                    && parts.next() == Some("__mod_init_func")
                    && matches!(parts.next(), None | Some("mod_init_funcs"))
            }
            BinaryFormat::Elf | BinaryFormat::Wasm => section == ".init_array",
            _ => false,
        };
        let mut ctors: Vec<RustcDefId> = vec![];
        let mut add_if_ctor = |def_id: RustcDefId, used: bool| {
            if used
                && let Some(section) = tcx.codegen_fn_attrs(def_id).link_section
                && is_ctor_section(section.as_str())
            {
                ctors.push(def_id);
            }
        };
        // For the local crate, we walk all its items and check their `#[used]` flags. We can't do
        // like below because if the crate is a binary its exported symbols don't include its
        // `#[used]` statics.
        for ldid in tcx.hir_crate_items(()).definitions() {
            if tcx.def_kind(ldid).has_codegen_attrs() && !tcx.is_foreign_item(ldid) {
                let flags = tcx.codegen_fn_attrs(ldid).flags;
                let used = flags.intersects(
                    CodegenFnAttrFlags::USED_COMPILER | CodegenFnAttrFlags::USED_LINKER,
                );
                add_if_ctor(ldid.to_def_id(), used);
            }
        }
        // For binary crates, we also look at the exported symbols of the dependencies, because
        // their constructors get linked into the final executable.
        if tcx.crate_types().contains(&CrateType::Executable) {
            for &cnum in tcx.crates(()) {
                if tcx.crate_dep_kind(cnum).macros_only() {
                    continue;
                }
                for &(symbol, info) in tcx.exported_non_generic_symbols(cnum) {
                    if let ExportedSymbol::NonGeneric(def_id) = symbol
                        && !tcx.is_foreign_item(def_id)
                    {
                        add_if_ctor(def_id, info.used);
                    }
                }
            }
        }

        ctors
            .into_iter()
            .filter_map(|def_id| {
                let item = rustc_public::CrateItem(rustc_internal::stable(def_id));
                let stt = mir::mono::StaticDef::try_from(item).ok()?;
                Some(self.register_global_from_static(&None, stt))
            })
            .collect()
    }
}

/// The functions of the crate and its dependencies exported under an explicit symbol
/// (`#[no_mangle]` or `#[export_name]`), by symbol: the definitions the linker binds the foreign
/// items with that symbol to, as Miri's `lookup_exported_symbol` finds them. Some definitions are
/// left to consumers, which model what they stand for at the foreign item:
/// - weak ones (`#[linkage = "weak"]`, e.g. `compiler_builtins`' fallbacks for the C math
///   library), which the system's definitions take precedence over;
/// - the standard library's internal symbols (`#[rustc_std_internal_symbol]`: the default
///   allocator, the panic runtime, ...), which implement what the compiler provides. Those the
///   program defines through an attribute are kept (see `is_linked_internal_fn`).
fn exported_fns(
    tcx: TyCtxt<'_>,
) -> std::collections::HashMap<String, Vec<rustc_span::def_id::DefId>> {
    use rustc_hir::attrs::Linkage;
    use rustc_hir::def::DefKind;
    use rustc_middle::middle::codegen_fn_attrs::CodegenFnAttrFlags;
    use rustc_middle::middle::exported_symbols::ExportedSymbol;

    let local = tcx
        .hir_crate_items(())
        .definitions()
        .map(|ldid| ldid.to_def_id());
    let dependencies = tcx
        .crates(())
        .iter()
        .filter(|&&cnum| !tcx.crate_dep_kind(cnum).macros_only())
        .flat_map(|&cnum| tcx.exported_non_generic_symbols(cnum))
        .filter_map(|(symbol, _)| match symbol {
            ExportedSymbol::NonGeneric(def_id) => Some(*def_id),
            _ => None,
        });
    local
        .chain(dependencies)
        .filter(|&def_id| {
            matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn)
                && !tcx.is_foreign_item(def_id)
                && !tcx.generics_of(def_id).requires_monomorphization(tcx)
        })
        .filter(|&def_id| {
            let attrs = tcx.codegen_fn_attrs(def_id);
            let explicit = (attrs.flags.contains(CodegenFnAttrFlags::NO_MANGLE)
                || attrs.symbol_name.is_some())
                && !attrs
                    .flags
                    .contains(CodegenFnAttrFlags::RUSTC_STD_INTERNAL_SYMBOL);
            (explicit || is_linked_internal_fn(tcx, def_id))
                && matches!(attrs.linkage, None | Some(Linkage::External))
        })
        .into_group_map_by(|&def_id| {
            let instance = rustc_middle::ty::Instance::mono(tcx, def_id);
            tcx.symbol_name(instance).name.to_owned()
        })
}

/// Whether the function `def_id` is a standard library's internal symbol that the program
/// defines through an attribute, which foreign items link to:
/// - the global allocator's functions: `#[global_allocator]` defines `__rust_alloc` and the other
///   allocator functions (marked `#[rustc_allocator]`, ...), which the program's allocations go
///   through;
/// - the panic handler (`#[panic_handler]`, std's unless the crate is `#![no_std]`): the
///   definition of the `panic_impl` lang item, `rust_begin_unwind`, which `core`'s panics call
///   through their foreign `panic_impl`.
fn is_linked_internal_fn(tcx: TyCtxt<'_>, def_id: rustc_span::def_id::DefId) -> bool {
    use rustc_middle::middle::codegen_fn_attrs::CodegenFnAttrFlags;
    let attrs = tcx.codegen_fn_attrs(def_id);
    let global_allocator = attrs.flags.intersects(
        CodegenFnAttrFlags::ALLOCATOR
            | CodegenFnAttrFlags::DEALLOCATOR
            | CodegenFnAttrFlags::REALLOCATOR
            | CodegenFnAttrFlags::ALLOCATOR_ZEROED,
    );
    let panic_handler = tcx.lang_items().panic_impl() == Some(def_id);
    attrs
        .flags
        .contains(CodegenFnAttrFlags::RUSTC_STD_INTERNAL_SYMBOL)
        && (global_allocator || panic_handler)
}

/// The symbol of `def_id` if it is one of the standard library's internal symbols the program
/// defines (see `is_linked_internal_fn`), which foreign functions link to.
pub(crate) fn linked_internal_symbol(
    tcx: TyCtxt<'_>,
    def_id: rustc_span::def_id::DefId,
) -> Option<String> {
    let is_fn = matches!(
        tcx.def_kind(def_id),
        rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn
    );
    (is_fn
        && !tcx.is_foreign_item(def_id)
        && !tcx.generics_of(def_id).requires_monomorphization(tcx)
        && is_linked_internal_fn(tcx, def_id))
    .then(|| {
        let instance = rustc_middle::ty::Instance::mono(tcx, def_id);
        tcx.symbol_name(instance).name.to_owned()
    })
}

pub(crate) const FAKE_DYN_TRAIT: TraitDeclId = TraitDeclId::ZERO;

/// Describe the target platform we're compiling for. Mirrors charon's `register_target_info`.
fn target_info(ctx: &mut TranslateCtx<'_>) -> TargetInfo {
    let tcx = ctx.tcx;
    let target_data = &tcx.data_layout;

    let mut primitive_alignments = SeqHashMap::new();
    primitive_alignments.insert(ScalarTy::Bool, target_data.i8_align.bytes());
    for (ty, alignment) in [
        (IntegerTy::Signed(IntTy::I8), target_data.i8_align.bytes()),
        (IntegerTy::Signed(IntTy::I16), target_data.i16_align.bytes()),
        (IntegerTy::Signed(IntTy::I32), target_data.i32_align.bytes()),
        (IntegerTy::Signed(IntTy::I64), target_data.i64_align.bytes()),
        (
            IntegerTy::Signed(IntTy::I128),
            target_data.i128_align.bytes(),
        ),
        (
            IntegerTy::Signed(IntTy::Isize),
            target_data.pointer_align().bytes(),
        ),
        (
            IntegerTy::Unsigned(UIntTy::U8),
            target_data.i8_align.bytes(),
        ),
        (
            IntegerTy::Unsigned(UIntTy::U16),
            target_data.i16_align.bytes(),
        ),
        (
            IntegerTy::Unsigned(UIntTy::U32),
            target_data.i32_align.bytes(),
        ),
        (
            IntegerTy::Unsigned(UIntTy::U64),
            target_data.i64_align.bytes(),
        ),
        (
            IntegerTy::Unsigned(UIntTy::U128),
            target_data.i128_align.bytes(),
        ),
        (
            IntegerTy::Unsigned(UIntTy::Usize),
            target_data.pointer_align().bytes(),
        ),
    ] {
        primitive_alignments.insert(ScalarTy::Integer(ty), alignment);
    }
    primitive_alignments.insert(ScalarTy::Float(FloatTy::F16), target_data.f16_align.bytes());
    primitive_alignments.insert(ScalarTy::Float(FloatTy::F32), target_data.f32_align.bytes());
    primitive_alignments.insert(ScalarTy::Float(FloatTy::F64), target_data.f64_align.bytes());
    primitive_alignments.insert(
        ScalarTy::Float(FloatTy::F128),
        target_data.f128_align.bytes(),
    );
    // INFO: This is not explicitly guaranteed by the reference, but by the implementation of rustc.
    // https://doc.rust-lang.org/1.97.1/nightly-rustc/src/rustc_ty_utils/layout.rs.html#391
    primitive_alignments.insert(ScalarTy::Char, target_data.i32_align.bytes());

    let c_enum_smallest_repr_ty = match target_data.c_enum_min_size {
        rustc_abi::Integer::I8 => IntTy::I8,
        rustc_abi::Integer::I16 => IntTy::I16,
        rustc_abi::Integer::I32 => IntTy::I32,
        rustc_abi::Integer::I64 => IntTy::I64,
        rustc_abi::Integer::I128 => IntTy::I128,
    };

    let target = &tcx.sess.target;
    TargetInfo {
        arch: ctx
            .translate_rustc_arch(&target.arch)
            .expect("all rustc Arch variants should be translated"),
        vendor: target.vendor_symbol().to_string(),
        os: ctx
            .translate_rustc_os(&target.os)
            .expect("all rustc Os variants should be translated"),
        env: ctx
            .translate_rustc_env(&target.env)
            .expect("all rustc Env variants should be translated"),
        cfg_abi: ctx
            .translate_rustc_cfg_abi(&target.cfg_abi)
            .expect("all rustc CfgAbi variants should be translated"),
        binary_format: ctx
            .translate_rustc_binary_format(&target.binary_format)
            .expect("all rustc BinaryFormat variants should be translated"),
        target_pointer_size: target_data.pointer_size().bytes(),
        is_little_endian: matches!(target_data.endian, rustc_abi::Endian::Little),
        c_enum_smallest_repr_ty,
        primitive_alignments,
    }
}

pub fn translate<'tcx, 'ctx>(
    options: &CliOpts,
    tcx: TyCtxt<'tcx>,
    sysroot: PathBuf,
) -> TransformCtx {
    // Retrieve the crate name: if the user specified a custom name, use it, otherwise retrieve it
    // from hax.
    let krate = rustc_public::local_crate();

    let charon_opts = CharonCliOpts {
        opaque: options.opaque.clone(),
        extract_opaque_bodies: true,
        reconstruct_asserts: true,
        raw_consts: false,
        reconstruct_fallible_operations: false,
        start_from: vec!["*".into()],
        ..CharonCliOpts::default()
    };
    let mut error_ctx = ErrorCtx::new();
    error_ctx.continue_on_failure = !charon_opts.abort_on_error;
    error_ctx.error_on_warnings = charon_opts.error_on_warnings;
    let translate_options = TranslateOptions::new(&mut error_ctx, &charon_opts);
    let mut ctx = TranslateCtx {
        tcx,
        options: translate_options,
        sysroot,
        errors: RefCell::new(error_ctx),
        translated: TranslatedCrate {
            crate_name: krate.name,
            options: charon_opts.clone(),
            runtime_checks: RuntimeChecks {
                ub_checks: tcx.sess.ub_checks(),
                overflow_checks: tcx.sess.overflow_checks(),
                contract_checks: tcx.sess.contract_checks(),
            },
            ..TranslatedCrate::default()
        },
        id_map: Default::default(),
        reverse_id_map: Default::default(),
        file_to_id: Default::default(),
        source_file_to_id: Default::default(),
        span_cache: Default::default(),
        items_to_translate: Default::default(),
        processed: Default::default(),
        cached_item_metas: Default::default(),
        cached_names: Default::default(),
        type_trans_cache: Default::default(),
        alloc_types: Default::default(),
        test_functions: Default::default(),
        exported_fns: None,
        linked_definitions: Default::default(),
    };

    let triple = ctx.get_target_triple();
    let target_info = target_info(&mut ctx);
    ctx.translated
        .target_information
        .insert(triple, target_info);

    // When building a test binary, find the `#[test]` functions, which are entry points and get
    // back a `test` attribute (see `translate_attr_info`).
    ctx.test_functions = ctx.collect_test_functions();

    ctx.reserve_unit_decl();
    ctx.translate_fake_dyn_trait();

    let mut entrypoints = ctx.collect_entrypoints(options);
    entrypoints.extend(ctx.collect_global_ctors().into_iter().map(ItemId::Global));

    // Translate.
    //
    // For as long as the queue of items to translate is not empty, we pop the top item and
    // translate it. If an item refers to non-translated (potentially external) items, we add them
    // to the queue.
    //
    // Note that the order in which we translate the definitions doesn't matter:
    // we never need to lookup a translated definition, and only use the map
    // from Rust ids to translated ids.
    //
    // Polymorphic items registered only so a name could refer to them are never enqueued, so they
    // have no name yet once the queue drains. We name each one (which may enqueue further items, or
    // register more name-only ones), alternating draining and name-filling until both are stable.
    loop {
        while let Some((_, item_id)) = ctx.items_to_translate.pop_first() {
            let item_src = ctx.reverse_id_map.get(&item_id).unwrap().clone();
            trace!("About to translate item: {:?}", item_src);
            if ctx.processed.insert(item_src.clone()) {
                ctx.translate_item(&item_src);
            }
        }

        let missing: Vec<ItemId> = ctx
            .reverse_id_map
            .keys()
            .copied()
            .filter(|id| !ctx.translated.item_names.contains_key(id))
            .sorted()
            .collect();
        if missing.is_empty() {
            break;
        }
        for id in missing {
            let src = ctx.reverse_id_map.get(&id).unwrap().clone();
            let name = ctx.translate_name(&src).unwrap_or_else(|_| Name {
                name: vec![PathElem::Ident("unknown".into(), Disambiguator::ZERO)],
            });
            ctx.translated.item_names.insert(id, name);
        }
    }

    // Later passes (e.g. `reorder_decls`) keep only the items reachable from these. Linked
    // definitions are only reachable through their symbol.
    let linked = std::mem::take(&mut ctx.linked_definitions);
    for id in entrypoints.into_iter().chain(linked) {
        if let Some(mut item) = ctx.translated.get_item_mut(id) {
            item.item_meta().started_from = true;
        }
    }

    // Return the context, dropping the hax state and rustc `tcx`.
    TransformCtx {
        options: ctx.options,
        translated: ctx.translated,
        errors: ctx.errors,
    }
}
