extern crate rustc_middle;
extern crate rustc_public;
extern crate rustc_trait_selection;

use super::translate_ctx::*;

use charon_lib::ast::*;
use rustc_middle::ty as mty;
use rustc_public::{mir::mono::Instance, ty};

impl<'tcx, 'ctx> ItemTransCtx<'tcx, 'ctx> {
    /// The erased pointer type that makes up a vtable entry: `*const ()`.
    fn vtable_entry_ty() -> Ty {
        TyKind::RawPtr(Ty::mk_unit(), RefKind::Shared).into_ty()
    }

    /// The rustc vtable entries for the given (possibly absent) principal trait.
    fn vtable_entries(principal: &Option<ty::TraitRef>) -> Vec<ty::VtblEntry> {
        if let Some(trait_ref) = principal {
            trait_ref.vtable_entries()
        } else {
            rustc_public::rustc_internal::stable(mty::TyCtxt::COMMON_VTABLE_ENTRIES)
        }
    }

    /// The type of the vtable global for this principal trait. We don't declare vtable structs
    /// like charon does; instead, a vtable follows the layout rustc uses: an array of erased
    /// pointers `[*const (); N]` (drop, size, align, then the methods and supertrait vtables).
    fn vtable_global_ty(principal: &Option<ty::TraitRef>) -> Ty {
        let len = ConstantExpr::mk_usize(Self::vtable_entries(principal).len() as u128);
        Ty::mk_array(Self::vtable_entry_ty(), len, None)
    }

    /// A constant pointer to the vtable of `ty` for the given principal trait, erased to
    /// `*const ()`. This is the pointer metadata of a `*const dyn Trait` built from `ty`.
    pub(crate) fn translate_vtable_ptr_const(
        &mut self,
        span: Span,
        ty: ty::Ty,
        principal: Option<ty::TraitRef>,
    ) -> ConstantExpr {
        let global_ty = Self::vtable_global_ty(&principal);
        let id = self.register_vtable(span, ty, principal);
        let global = GlobalPlace::new_global(
            GlobalDeclRef {
                id,
                generics: Box::new(GenericArgs::empty()),
            },
            global_ty.clone(),
        );
        let ptr = ConstantExpr::new(
            ConstantExprKind::Ptr(RefKind::Shared, global, None),
            TyKind::RawPtr(global_ty, RefKind::Shared).into_ty(),
        );
        let erased_ty = Self::vtable_entry_ty();
        ConstantExpr::new(
            ConstantExprKind::Cast(ptr, CastKind::RawPtr(erased_ty.clone())),
            erased_ty,
        )
    }

    /// A function pointer to the given instance, erased to `*const ()` (like charon's
    /// monomorphic vtable method entries).
    fn erased_fn_ptr_const(
        &mut self,
        span: Span,
        instance: Instance,
    ) -> Result<ConstantExpr, Error> {
        let sig = self.translate_function_signature(instance, span)?;
        let fun = self.register_fun_decl_id(span, instance);
        let fn_ptr = FnPtr {
            kind: Box::new(FnPtrKind::Fun(fun)),
            generics: Box::new(GenericArgs::empty()),
        };
        let fn_ptr_ty = TyKind::FnPtr(RegionBinder::empty(sig)).into_ty();
        let fn_ptr = ConstantExpr::new(ConstantExprKind::FnPtr(fn_ptr), fn_ptr_ty);
        let erased_ty = Self::vtable_entry_ty();
        Ok(ConstantExpr::new(
            ConstantExprKind::Cast(fn_ptr, CastKind::RawPtr(erased_ty.clone())),
            erased_ty,
        ))
    }

    /// Translate the vtable of `from_ty` for the given principal trait. Like charon's vtable
    /// instances, its value is a constant; it is an array of erased pointers laid out like rustc
    /// lays out vtables:
    ///
    /// ```text
    /// [
    ///   drop_in_place::<T> as *const (),
    ///   size_of::<T>() as *const (),
    ///   align_of::<T>() as *const (),
    ///   method_1 as *const (),
    ///   ...
    ///   &super_trait_vtable as *const (),
    ///   ...
    /// ]
    /// ```
    pub fn translate_vtable(
        mut self,
        def_id: GlobalDeclId,
        item_meta: ItemMeta,
        from_ty: ty::Ty,
        principal: Option<ty::TraitRef>,
    ) -> Result<GlobalDecl, Error> {
        let span = item_meta.span;
        let entry_ty = Self::vtable_entry_ty();
        // Sizes and alignments are stored as addresses without provenance.
        let mk_usize_entry = |n: usize| {
            ConstantExpr::new(
                ConstantExprKind::PtrNoProvenance(n as u128),
                entry_ty.clone(),
            )
        };

        use ty::VtblEntry::*;
        let layout = from_ty.layout()?.shape();
        let entries: Vec<ConstantExpr> = Self::vtable_entries(&principal)
            .into_iter()
            .map(|vtable_entry| match vtable_entry {
                MetadataDropInPlace => {
                    let drop = Instance::resolve_drop_in_place(from_ty);
                    self.erased_fn_ptr_const(span, drop)
                }
                MetadataSize => Ok(mk_usize_entry(layout.size.bytes())),
                MetadataAlign => Ok(mk_usize_entry(layout.abi_align as usize)),
                Method(instance) => self.erased_fn_ptr_const(span, instance),
                TraitVPtr(super_trait) => {
                    Ok(self.translate_vtable_ptr_const(span, from_ty, Some(super_trait)))
                }
                // Rustc leaves vacant entries (e.g. for methods that require `Self: Sized`) as
                // null pointers; keep them so that the entry indices match rustc's.
                Vacant => Ok(mk_usize_entry(0)),
            })
            .try_collect()?;

        let ty = Self::vtable_global_ty(&principal);
        let value = ConstantExpr::new(ConstantExprKind::Array(entries), ty.clone());
        let self_ty = self.translate_ty(span, from_ty)?;
        let impl_ref = TraitImplRef {
            id: self.register_vtable_impl(span, from_ty, principal),
            generics: Box::new(GenericArgs::empty()),
        };
        Ok(GlobalDecl {
            def_id,
            item_meta,
            generics: GenericParams::empty(),
            size: Size::from_expr(SizeExpr::size_of(&ty)),
            align: Size::from_expr(SizeExpr::align_of(&ty)),
            ptr_metadata: ConstantExpr::mk_unit(),
            global_kind: GlobalKind::VTable,
            src: GlobalSource::VTableInstance {
                self_ty,
                impl_ref: Some(impl_ref),
            },
            ty,
            value,
        })
    }
    /// The trait implementation the vtable of `self_ty` for `principal` is for: an impl of the
    /// principal trait for `self_ty`, or of `FakeTrait` without principal trait (the trait of
    /// trait objects without principal, see `translate_dyn_ty`). This records which trait a vtable
    /// is for, as charon's vtable instances do with their originating impl. Like Obol's other
    /// trait impls, it omits the implemented items. Its implied trait refs are the impls of the
    /// vtables the trait objects upcast to (see `supertraits`), and of `FakeTrait`; they don't
    /// follow the (empty) implied clauses of Obol's trait declarations.
    pub fn translate_vtable_impl(
        mut self,
        def_id: TraitImplId,
        item_meta: ItemMeta,
        self_ty: ty::Ty,
        principal: Option<ty::TraitRef>,
    ) -> Result<TraitImpl, Error> {
        let span = item_meta.span;
        let impl_trait = self.translate_principal_ref(span, self_ty, &principal)?;
        let vtable = GlobalDeclRef {
            id: self.register_vtable(span, self_ty, principal.clone()),
            generics: Box::new(GenericArgs::empty()),
        };
        let upcasts = match &principal {
            Some(principal) => {
                let mut upcasts = self.supertraits(principal);
                upcasts.push(None);
                upcasts
            }
            None => vec![],
        };
        let implied_trait_refs = upcasts
            .into_iter()
            .map(|target| {
                let trait_decl_ref = self.translate_principal_ref(span, self_ty, &target)?;
                let impl_ref = TraitImplRef {
                    id: self.register_vtable_impl(span, self_ty, target),
                    generics: Box::new(GenericArgs::empty()),
                };
                let kind = TraitRefKind::TraitImpl(impl_ref);
                Ok::<_, Error>(PolyTraitRef::empty(TraitRef::new(kind, trait_decl_ref)))
            })
            .try_collect()?;
        Ok(TraitImpl {
            def_id,
            item_meta,
            src: TraitImplSource::Normal,
            impl_trait,
            is_negative: false,
            is_unsafe: false,
            generics: GenericParams::empty(),
            implied_trait_refs,
            consts: IndexMap::new(),
            types: IndexMap::new(),
            methods: IndexMap::new(),
            vtable: VTableDecl::VTable(vtable),
        })
    }

    /// The principal trait `principal` of a vtable of `self_ty`, or `FakeTrait` (applied to
    /// `self_ty`) if there is none.
    fn translate_principal_ref(
        &mut self,
        span: Span,
        self_ty: ty::Ty,
        principal: &Option<ty::TraitRef>,
    ) -> Result<TraitDeclRef, Error> {
        Ok(match principal {
            Some(principal) => TraitDeclRef {
                id: self.register_trait_decl_id(span, principal.def_id),
                generics: Box::new(self.translate_generic_args(span, principal.args())?),
            },
            None => TraitDeclRef {
                generics: Box::new(GenericArgs::new_types(
                    [self.translate_ty(span, self_ty)?].into(),
                )),
                ..self.dummy_trait_decl_ref()
            },
        })
    }

    /// The transitive supertraits of `trait_ref` that trait objects can upcast to: neither auto
    /// traits nor sizedness traits.
    fn supertraits(&self, trait_ref: &ty::TraitRef) -> Vec<Option<ty::TraitRef>> {
        let tcx = self.t_ctx.tcx;
        let internal = rustc_public::rustc_internal::internal(tcx, trait_ref);
        rustc_trait_selection::traits::supertraits(tcx, mty::Binder::dummy(internal))
            .map(|sup| tcx.instantiate_bound_regions_with_erased(sup))
            .filter(|sup| {
                *sup != internal
                    && !tcx.trait_is_auto(sup.def_id)
                    && !tcx.is_sizedness_trait(sup.def_id)
            })
            .map(|sup| Some(rustc_public::rustc_internal::stable(sup)))
            .collect()
    }
}
