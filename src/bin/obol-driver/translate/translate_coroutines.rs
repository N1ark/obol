//! Coroutines, i.e. the state machines of `async fn`s and `async` blocks.
//!
//! The AST has no coroutine type, so we declare each (monomorphic) coroutine as a struct
//! ```text
//! struct {coroutine} { upvar_0: U_0, .., __state: uN, slot_0: MaybeUninit<T_0>, .. }
//! ```
//! with one slot per local that rustc saves across suspension points. The struct gets rustc's
//! layout, in which the slots of different suspension points overlap: rustc computes sizes of
//! coroutines (e.g. for `Box::pin`) with that layout. Like rustc does to compute it, we wrap the
//! saved locals in `MaybeUninit`: a slot is uninitialized whenever the coroutine is in a state
//! that doesn't use it.
//!
//! Bodies (the resume function and the drop glue) are rewritten accordingly:
//! - `c = Coroutine(op_0, ..)` becomes `c.upvar_0 = op_0; ..; c.__state = 0`;
//! - `(c as variant#v).k` becomes `c.slot_j.value`, where `j` is the `k`-th local saved by `v`;
//! - `discriminant(c)` becomes `copy c.__state as u32`;
//! - `SetDiscriminant(c, v)` becomes `c.__state = v`.
extern crate rustc_abi;
extern crate rustc_middle;
extern crate rustc_public;

use rustc_public::{CrateDef, CrateDefType, rustc_internal, ty};

use charon_lib::{ast::*, ids::IndexVec, raise_error, register_error, ullbc_ast::*};

use crate::translate::translate_ctx::ItemTransCtx;
use crate::translate::translate_types::inhabited_predicate;

/// The shape of a monomorphic coroutine.
pub(crate) struct CoroutineInfo {
    upvars: Vec<ty::Ty>,
    /// The types of the slots, `MaybeUninit<T>` for each saved local of type `T`.
    slots: Vec<ty::Ty>,
    /// `variant_fields[v][k]` is the saved local stored as the `k`-th field of variant `v`.
    variant_fields: Vec<Vec<usize>>,
    /// The type of the state; rustc picks the smallest integer that fits all variants.
    state_ty: UIntTy,
    layout: Layout,
}

impl CoroutineInfo {
    fn state_field(&self) -> FieldId {
        FieldId::from_usize(self.upvars.len())
    }

    fn slot_field(&self, slot: usize) -> FieldId {
        FieldId::from_usize(self.upvars.len() + 1 + slot)
    }

    fn state_ty(&self) -> Ty {
        TyKind::Scalar(ScalarTy::Integer(IntegerTy::Unsigned(self.state_ty))).into_ty()
    }

    /// The `__state` field of the coroutine at `place`.
    pub(crate) fn state_place(&self, place: Place) -> Place {
        place.project(
            ProjectionElem::Field(None, self.state_field()),
            self.state_ty(),
        )
    }

    /// The state of the given variant, i.e. its index.
    pub(crate) fn state_of_variant(&self, variant: usize) -> Operand {
        Operand::Const(ConstantExpr::new(
            ConstantExprKind::Integer(IntegerValue::Unsigned(self.state_ty, variant as u128)),
            self.state_ty(),
        ))
    }

    /// Read the state as the coroutine's discriminant, which is a `u32`.
    pub(crate) fn discriminant(&self, place: Place) -> Rvalue {
        let state = Operand::Copy(self.state_place(place));
        let src = ScalarTy::Integer(IntegerTy::Unsigned(self.state_ty));
        let tgt = ScalarTy::Integer(IntegerTy::Unsigned(UIntTy::U32));
        if src == tgt {
            Rvalue::Use(state, WithRetag::No)
        } else {
            Rvalue::UnaryOp(UnOp::Cast(CastKind::Scalar(src, tgt)), state)
        }
    }
}

/// The definition and arguments of a coroutine type.
pub(crate) fn as_coroutine(ty: ty::Ty) -> Option<(ty::CoroutineDef, ty::GenericArgs)> {
    match ty.kind() {
        ty::TyKind::RigidTy(ty::RigidTy::Coroutine(def, args)) => Some((def, args)),
        _ => None,
    }
}

impl ItemTransCtx<'_, '_> {
    pub(crate) fn coroutine_info(
        &mut self,
        span: Span,
        def: ty::CoroutineDef,
        args: &ty::GenericArgs,
    ) -> Result<CoroutineInfo, Error> {
        use rustc_abi as r_abi;
        use rustc_middle::ty as rustc_ty;

        let tcx = self.t_ctx.tcx;
        let typing_env = rustc_ty::TypingEnv::fully_monomorphized();
        let def_id = rustc_internal::internal(tcx, def.def_id());
        let args = rustc_internal::internal(tcx, args);

        let upvars: Vec<ty::Ty> = args
            .as_coroutine()
            .upvar_tys()
            .iter()
            .map(rustc_internal::stable)
            .collect();
        let Ok(coroutine_layout) = tcx.coroutine_layout(def_id, args) else {
            raise_error!(self, span, "Could not compute the layout of a coroutine");
        };
        let mut slots = vec![];
        for saved in &coroutine_layout.field_tys {
            let saved_ty = rustc_ty::EarlyBinder::bind(tcx, saved.ty);
            let Ok(saved_ty) =
                tcx.try_instantiate_and_normalize_erasing_regions(args, typing_env, saved_ty)
            else {
                raise_error!(self, span, "Could not normalize a coroutine's saved local");
            };
            // `rustc_public` panics on async closure types.
            if saved_ty
                .walk()
                .filter_map(|arg| arg.as_type())
                .any(|ty| matches!(ty.kind(), rustc_ty::CoroutineClosure(..)))
            {
                raise_error!(self, span, "Async closures are not supported yet");
            }
            let slot_ty = rustc_ty::Ty::new_maybe_uninit(tcx, saved_ty);
            slots.push(rustc_internal::stable(slot_ty));
        }
        let variant_fields: Vec<Vec<usize>> = coroutine_layout
            .variant_fields
            .iter()
            .map(|fields| fields.iter().map(|local| local.as_usize()).collect())
            .collect();

        let coroutine_ty = rustc_ty::Ty::new_coroutine(tcx, def_id, args);
        let Ok(layout) = tcx
            .layout_of(typing_env.as_query_input(coroutine_ty))
            .map(|layout| layout.layout)
        else {
            raise_error!(self, span, "Could not compute the layout of a coroutine");
        };
        let (
            r_abi::Variants::Multiple {
                tag,
                tag_field,
                variants,
                ..
            },
            r_abi::FieldsShape::Arbitrary { offsets, .. },
        ) = (layout.variants(), layout.fields())
        else {
            raise_error!(self, span, "Unexpected coroutine layout");
        };
        let state_ty = match tag.primitive() {
            r_abi::Primitive::Int(r_abi::Integer::I8, false) => UIntTy::U8,
            r_abi::Primitive::Int(r_abi::Integer::I16, false) => UIntTy::U16,
            r_abi::Primitive::Int(r_abi::Integer::I32, false) => UIntTy::U32,
            _ => raise_error!(self, span, "Unexpected coroutine tag"),
        };
        // The prefix holds the upvars then the tag; a variant's fields are the slots it uses.
        let mut field_offsets: IndexVec<FieldId, OffsetExpr> = offsets
            .iter()
            .take(upvars.len())
            .map(|o| OffsetExpr::new(o.bytes()))
            .collect();
        field_offsets.push(OffsetExpr::new(offsets[*tag_field].bytes()));
        let mut slot_offsets = vec![None; slots.len()];
        for (fields, variant) in variant_fields.iter().zip(variants) {
            for (local, offset) in fields.iter().zip(&variant.field_offsets) {
                slot_offsets[*local] = Some(offset.bytes());
            }
        }
        for offset in slot_offsets {
            let Some(offset) = offset else {
                raise_error!(self, span, "A coroutine's saved local is in no variant");
            };
            field_offsets.push(OffsetExpr::new(offset));
        }
        let layout = Layout {
            size: Size::new(layout.size().bytes()),
            align: Size::new(layout.align().abi.bytes()),
            discriminator: Some(Discriminator::trivial(VariantId::ZERO)),
            inhabited: inhabited_predicate(!layout.is_uninhabited()),
            variant_layouts: [Some(VariantLayout {
                field_offsets,
                inhabited: inhabited_predicate(!layout.is_uninhabited()),
                tagger: vec![],
            })]
            .into_iter()
            .collect(),
            repr: ReprOptions::default(),
        };

        Ok(CoroutineInfo {
            upvars,
            slots,
            variant_fields,
            state_ty,
            layout,
        })
    }

    /// The place of the `k`-th field of variant `v` of the coroutine at `place`, i.e. of the value
    /// held by the slot of the corresponding saved local.
    pub(crate) fn coroutine_saved_local(
        &mut self,
        span: Span,
        place: Place,
        info: &CoroutineInfo,
        variant: VariantId,
        k: usize,
        saved_ty: ty::Ty,
    ) -> Result<Place, Error> {
        let Some(&slot) = info
            .variant_fields
            .get(variant.index())
            .and_then(|fields| fields.get(k))
        else {
            raise_error!(self, span, "Unknown field of a coroutine variant");
        };
        let mut ty = info.slots[slot];
        let mut place = place.project(
            ProjectionElem::Field(None, info.slot_field(slot)),
            self.translate_ty(span, ty)?,
        );
        // The `value` field of `MaybeUninit<T>` holds the `T` within transparent wrappers
        // (`ManuallyDrop`, ...), whose first field we follow.
        let mut field = 1;
        while ty != saved_ty {
            let Some(ty::RigidTy::Adt(adt, args)) = ty.kind().rigid().cloned() else {
                raise_error!(self, span, "Unexpected coroutine slot type");
            };
            ty = adt.variants()[0].fields()[field].ty_with_args(&args);
            place = place.project(
                ProjectionElem::Field(None, FieldId::new(field)),
                self.translate_ty(span, ty)?,
            );
            field = 0;
        }
        Ok(place)
    }

    /// The statements that initialize a coroutine at `place` with the given upvars, in its initial
    /// state.
    pub(crate) fn coroutine_init(
        &mut self,
        span: Span,
        place: Place,
        info: &CoroutineInfo,
        upvars: Vec<Operand>,
    ) -> Result<Vec<StatementKind>, Error> {
        let mut statements = vec![];
        for (i, (upvar, ty)) in upvars.into_iter().zip(&info.upvars).enumerate() {
            let ty = self.translate_ty(span, *ty)?;
            let field = place
                .clone()
                .project(ProjectionElem::Field(None, FieldId::new(i)), ty);
            statements.push(StatementKind::Assign(
                field,
                Rvalue::Use(upvar, WithRetag::No),
            ));
        }
        statements.push(StatementKind::Assign(
            info.state_place(place),
            Rvalue::Use(info.state_of_variant(0), WithRetag::No),
        ));
        Ok(statements)
    }

    /// The fields of the struct we declare for a coroutine.
    fn translate_coroutine_fields(
        &mut self,
        span: Span,
        info: &CoroutineInfo,
    ) -> Result<IndexVec<FieldId, Field>, Error> {
        let mut fields: IndexVec<FieldId, Field> = Default::default();
        let mut push_field = |name: String, ty: Ty| {
            fields.push(Field {
                span,
                attr_info: AttrInfo::default(),
                name,
                is_positional: false,
                ty,
            })
        };
        for (i, ty) in info.upvars.iter().enumerate() {
            push_field(format!("upvar_{i}"), self.translate_ty(span, *ty)?);
        }
        push_field("__state".to_string(), info.state_ty());
        for (j, ty) in info.slots.iter().enumerate() {
            push_field(format!("slot_{j}"), self.translate_ty(span, *ty)?);
        }
        Ok(fields)
    }

    /// Translate a coroutine as a struct.
    pub(crate) fn translate_coroutine_adt(
        mut self,
        trans_id: TypeDeclId,
        item_meta: ItemMeta,
        def: &ty::CoroutineDef,
        args: &ty::GenericArgs,
    ) -> Result<TypeDecl, Error> {
        let span = item_meta.span;
        let (kind, layout) = match self
            .coroutine_info(span, *def, args)
            .and_then(|info| Ok((self.translate_coroutine_fields(span, &info)?, info.layout)))
        {
            Ok((fields, layout)) => {
                let target = self.t_ctx.get_target_triple();
                (TypeDeclKind::Struct(fields), [(target, layout)].into())
            }
            Err(err) => (TypeDeclKind::Error(err.msg), Default::default()),
        };
        let ty = ty::Ty::from_rigid_kind(ty::RigidTy::Coroutine(*def, args.clone()));
        Ok(TypeDecl {
            def_id: trans_id,
            item_meta,
            generics: GenericParams::empty(),
            kind,
            src: TypeSource::Normal,
            layout,
            ptr_metadata: self.translate_ptr_metadata(),
            marker_traits: self.translate_marker_traits(ty),
        })
    }
}
