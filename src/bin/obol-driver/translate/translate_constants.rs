extern crate rustc_apfloat;
extern crate rustc_middle;
extern crate rustc_public;
extern crate rustc_span;

use charon_lib::{ast::*, raise_error, register_error};
use itertools::Itertools;
use rustc_apfloat::{Float, ieee};
use rustc_const_uneval::{
    self as uneval, AllocTarget, ConstReader, ConstSource, GlobalRef, PtrTarget, ReadError,
    ReadMode,
};
use rustc_middle::ty as mty;
use rustc_public::rustc_internal::{internal, stable};
use rustc_public::{CrateDef, mir, ty};

use crate::translate::translate_ctx::ItemTransCtx;

impl<'tcx, 'ctx> ItemTransCtx<'tcx, 'ctx> {
    pub fn translate_tyconst_to_const_expr(
        &mut self,
        span: Span,
        v: &ty::TyConst,
    ) -> Result<ConstantExpr, Error> {
        if let ty::TyConstKind::Param(param) = v.kind() {
            // A free const parameter, only reachable while naming a polymorphic item. Emit a
            // const-generic variable rather than failing.
            let id = ConstGenericVarId::from_raw(param.index);
            let ty = TyKind::Error("Param type".to_string()).into_ty();
            register_error!(
                self,
                span,
                "translate const: got a const parameter: {:?}",
                param
            );
            return Ok(ConstantExpr::new(
                ConstantExprKind::Var(ConstGenericDbVar::Free(id)),
                ty,
            ));
        }
        let src = match internal(self.t_ctx.tcx, v).kind() {
            mty::ConstKind::Value(val) => ConstSource::ValTree(val),
            mty::ConstKind::Alias(_, uv) => ConstSource::TyConst(uv),
            _ => {
                raise_error!(
                    self,
                    span,
                    "Unexpected constant expression kind: {:?}",
                    v.kind()
                )
            }
        };
        self.read_const(span, src, ReadMode::Structured)
    }

    /// The reader for constants; we only see monomorphic ones.
    fn const_reader(&self) -> ConstReader<'tcx> {
        ConstReader {
            tcx: self.t_ctx.tcx,
            typing_env: mty::TypingEnv::fully_monomorphized(),
            config: uneval::Config {
                anon_allocs_as_globals: true,
                unsized_strings: false,
            },
            alloc_types: self.t_ctx.alloc_types.clone(),
        }
    }

    /// Evaluate `src` with rustc and read it back as a constant expression.
    pub(crate) fn read_const(
        &mut self,
        span: Span,
        src: ConstSource<'tcx>,
        mode: ReadMode,
    ) -> Result<ConstantExpr, Error> {
        match self.const_reader().read(rustc_span::DUMMY_SP, src, mode) {
            Ok(c) => self.translate_read_const(span, &c),
            Err(ReadError::NotEvaluable) => {
                raise_error!(self, span, "Could not evaluate constant: {src:?}")
            }
            Err(err) => raise_error!(self, span, "Could not read constant {src:?}: {err:?}"),
        }
    }

    /// Translate a constant read back by `rustc_const_uneval`.
    fn translate_read_const(
        &mut self,
        span: Span,
        c: &uneval::Const<'tcx>,
    ) -> Result<ConstantExpr, Error> {
        let ty = self.translate_ty(span, stable(c.ty))?;
        let kind = match &c.kind {
            uneval::ConstKind::Scalar(scalar) => {
                // The value of a pattern type is described at its base type.
                let base_ty = match ty.kind() {
                    TyKind::Pattern(base_ty, _) => base_ty.kind(),
                    kind => kind,
                };
                match base_ty {
                    TyKind::Scalar(ScalarTy::Bool) => {
                        ConstantExprKind::Bool(scalar.try_to_bool().unwrap())
                    }
                    TyKind::Scalar(ScalarTy::Char) => {
                        ConstantExprKind::Char(char::try_from(*scalar).unwrap())
                    }
                    TyKind::Scalar(ScalarTy::Integer(IntegerTy::Signed(ity))) => {
                        let value = scalar.to_int(scalar.size());
                        ConstantExprKind::Integer(IntegerValue::Signed(ity.clone(), value))
                    }
                    TyKind::Scalar(ScalarTy::Integer(IntegerTy::Unsigned(uty))) => {
                        let value = scalar.to_uint(scalar.size());
                        ConstantExprKind::Integer(IntegerValue::Unsigned(uty.clone(), value))
                    }
                    TyKind::Scalar(ScalarTy::Float(fty)) => {
                        let bits = scalar.to_bits_unchecked();
                        let value = match fty {
                            FloatTy::F16 => ieee::Half::from_bits(bits).to_string(),
                            FloatTy::F32 => ieee::Single::from_bits(bits).to_string(),
                            FloatTy::F64 => ieee::Double::from_bits(bits).to_string(),
                            FloatTy::F128 => ieee::Quad::from_bits(bits).to_string(),
                        };
                        ConstantExprKind::Float(FloatValue {
                            value,
                            bits,
                            ty: fty.clone(),
                        })
                    }
                    _ => raise_error!(self, span, "Unexpected type for a scalar constant: {ty:?}"),
                }
            }
            uneval::ConstKind::PtrNoProvenance(addr) => ConstantExprKind::PtrNoProvenance(*addr),
            uneval::ConstKind::Str(str) => ConstantExprKind::Str(str.clone()),
            uneval::ConstKind::ByteStr(bytes) => ConstantExprKind::ByteStr(bytes.clone()),
            uneval::ConstKind::Aggregate { variant, fields } => {
                let fields: Vec<_> = fields
                    .iter()
                    .map(|field| self.translate_read_const(span, field))
                    .try_collect()?;
                match c.ty.kind() {
                    mty::Array(..) | mty::Slice(..) => ConstantExprKind::Array(fields),
                    mty::Adt(adt_def, _) if adt_def.is_enum() => {
                        let variant = variant.map(|variant| self.translate_rvariant_id(variant));
                        ConstantExprKind::Adt(variant, fields.into_iter().collect())
                    }
                    _ => ConstantExprKind::Adt(None, fields.into_iter().collect()),
                }
            }
            uneval::ConstKind::FnDef { .. } => {
                let TyKind::FnDef(fn_ptr) = ty.kind() else {
                    unreachable!("Unexpected type for a function item: {ty:?}")
                };
                ConstantExprKind::FnDef(fn_ptr.skip_binder.clone())
            }
            uneval::ConstKind::FnPtr(target) => {
                ConstantExprKind::FnPtr(self.translate_fn_target(span, *target))
            }
            uneval::ConstKind::Ptr {
                target: PtrTarget::Inline(val),
                ..
            } if ty.is_ref()
                && let uneval::ConstKind::Str(str) = &val.kind =>
            {
                ConstantExprKind::Str(str.clone())
            }
            uneval::ConstKind::Ptr { target, unsize } => {
                let place = match target {
                    PtrTarget::Global {
                        target,
                        ty: global_ty,
                        projections,
                    } => {
                        let mut place = GlobalPlace {
                            kind: GlobalPlaceKind::Global(
                                self.translate_alloc_target(span, *target)?,
                            ),
                            ty: self.translate_ty(span, stable(*global_ty))?,
                        };
                        for (proj, proj_ty) in projections {
                            let proj = self.translate_const_projection(span, proj)?;
                            place = place.project(proj, self.translate_ty(span, stable(*proj_ty))?);
                        }
                        place
                    }
                    PtrTarget::Inline(val) => {
                        GlobalPlace::new_value(self.translate_read_const(span, val)?)
                    }
                };
                let metadata = self.translate_unsize(span, unsize)?;
                match ty.kind() {
                    TyKind::RawPtr(_, rk) => ConstantExprKind::Ptr(*rk, place, metadata),
                    _ => ConstantExprKind::Ref(place, metadata),
                }
            }
            uneval::ConstKind::PtrCast { ptr, unsize } => {
                let ptr = self.translate_read_const(span, ptr)?;
                let kind = match self.translate_unsize(span, unsize)? {
                    Some(metadata) => CastKind::Unsize(ty.clone(), metadata, vec![]),
                    None => CastKind::RawPtr(ty.clone()),
                };
                ConstantExprKind::Cast(ptr, kind)
            }
            uneval::ConstKind::Memory(bytes) => {
                // The bytes of a pointer share their provenance; we translate it only once.
                let mut last_prov: Option<(AllocTarget<'tcx>, GlobalProvenance)> = None;
                let mut charon_bytes = Vec::with_capacity(bytes.len());
                for byte in bytes {
                    charon_bytes.push(match *byte {
                        uneval::Byte::Uninit => Byte::Uninit,
                        uneval::Byte::Value(v) => Byte::Value(v),
                        uneval::Byte::Ptr(target, offset, i) => {
                            if last_prov.as_ref().is_none_or(|(last, _)| *last != target) {
                                let prov = self.translate_alloc_target(span, target)?;
                                last_prov = Some((target, prov));
                            }
                            let prov = last_prov.as_ref().unwrap().1.clone();
                            Byte::Provenance(
                                prov,
                                IntegerValue::mk_usize(offset.bytes() as u128),
                                i,
                            )
                        }
                    });
                }
                ConstantExprKind::RawMemory(charon_bytes)
            }
            uneval::ConstKind::TypeId(ty) => {
                ConstantExprKind::TypeId(self.translate_ty(span, stable(*ty))?)
            }
            uneval::ConstKind::Unsupported(msg) => {
                register_error!(self, span, "Unsupported constant: {msg}");
                ConstantExprKind::Opaque(msg.to_string())
            }
        };
        Ok(ConstantExpr::new(kind, ty))
    }

    fn translate_const_projection(
        &mut self,
        span: Span,
        proj: &uneval::Projection<'tcx>,
    ) -> Result<ConstProjectionElem, Error> {
        Ok(match *proj {
            uneval::Projection::Field(variant, field) => ConstProjectionElem::Field(
                variant.map(|variant| self.translate_rvariant_id(variant)),
                FieldId::new(field.as_usize()),
            ),
            uneval::Projection::Index(i) => {
                ConstProjectionElem::Index(IntegerValue::mk_usize(i as u128))
            }
            uneval::Projection::Subslice { from, to } => ConstProjectionElem::Subslice {
                from: IntegerValue::mk_usize(from as u128),
                to: IntegerValue::mk_usize(to as u128),
            },
            uneval::Projection::Offset {
                count,
                ty,
                in_bounds,
            } => ConstProjectionElem::Offset {
                count: IntegerValue::Signed(IntTy::Isize, count as i128),
                ty: ty
                    .map(|ty| self.translate_ty(span, stable(ty)))
                    .transpose()?,
                in_bounds,
                pointee_is_sized: None,
            },
        })
    }

    /// The metadata of a wide pointer whose pointee was unsized as described by `unsize`.
    fn translate_unsize(
        &mut self,
        span: Span,
        unsize: &Option<uneval::Unsize<'tcx>>,
    ) -> Result<Option<UnsizingMetadata>, Error> {
        let Some(unsize) = unsize else {
            return Ok(None);
        };
        let tcx = self.t_ctx.tcx;
        let ref_to = |ty| stable(mty::Ty::new_imm_ref(tcx, tcx.lifetimes.re_static, ty));
        let (path, metadata) =
            self.translate_unsizing_metadata(span, ref_to(unsize.from), ref_to(unsize.to))?;
        assert!(path.is_empty(), "Non-empty unsize path in const ptr");
        Ok(Some(metadata))
    }

    /// The function a function pointer points to.
    fn translate_fn_target(&mut self, span: Span, target: uneval::FnTarget<'tcx>) -> FnPtr {
        let id = match target {
            uneval::FnTarget::Instance(instance) => {
                self.register_fun_decl_id(span, stable(instance))
            }
            uneval::FnTarget::ClosureAsFn(def_id, args) => {
                let closure = mty::Ty::new_closure(self.t_ctx.tcx, def_id, args);
                let ty::TyKind::RigidTy(ty::RigidTy::Closure(closure, args)) =
                    stable(closure).kind()
                else {
                    unreachable!("Unexpected closure type: {closure:?}")
                };
                self.register_closure_as_fn_id(span, closure, args)
            }
        };
        FnPtr {
            kind: Box::new(FnPtrKind::Fun(id)),
            generics: Box::new(GenericArgs::empty()),
        }
    }

    /// The provenance of a pointer into the given allocation.
    fn translate_alloc_target(
        &mut self,
        span: Span,
        target: AllocTarget<'tcx>,
    ) -> Result<GlobalProvenance, Error> {
        let tcx = self.t_ctx.tcx;
        let id = match target {
            AllocTarget::Fn(target) => {
                let fn_ptr_ty = target.fn_ptr_ty(tcx, mty::TypingEnv::fully_monomorphized());
                let fn_ptr_ty = self.translate_ty(span, stable(fn_ptr_ty))?;
                let fn_ptr = self.translate_fn_target(span, target);
                return Ok(GlobalProvenance::Function(fn_ptr, fn_ptr_ty));
            }
            AllocTarget::Global(global) => {
                let alloc_id = match global {
                    GlobalRef::Static(def_id) => tcx.reserve_and_set_static_alloc(def_id),
                    GlobalRef::Alloc(alloc_id) => alloc_id,
                };
                let ty = stable(self.const_reader().global_ty(global));
                self.register_global_decl_id(span, stable(alloc_id), Some(ty))
            }
            AllocTarget::VTable(ty, preds) => {
                let self_ty = stable(ty);
                let principal = preds.principal().map(|principal| {
                    stable(tcx.instantiate_bound_regions_with_erased(principal))
                        .with_self_ty(self_ty)
                });
                self.register_vtable(span, self_ty, principal)
            }
            AllocTarget::TypeId(ty) => {
                return Ok(GlobalProvenance::TypeId(
                    self.translate_ty(span, stable(ty))?,
                ));
            }
        };
        Ok(GlobalProvenance::Global(GlobalDeclRef {
            id,
            generics: Box::new(GenericArgs::empty()),
        }))
    }

    /// The value of the given (non-foreign) static.
    pub(crate) fn read_static_value(
        &mut self,
        span: Span,
        def: mir::mono::StaticDef,
    ) -> Result<ConstantExpr, Error> {
        let def_id = internal(self.t_ctx.tcx, def.def_id());
        let src = ConstSource::Global(GlobalRef::Static(def_id));
        self.read_const(span, src, ReadMode::Structured)
    }

    pub(crate) fn translate_const_value(
        &mut self,
        span: Span,
        const_: &ty::MirConst,
    ) -> Result<ConstantExpr, Error> {
        let tcx = self.t_ctx.tcx;
        let const_ = internal(tcx, const_);
        let typing_env = mty::TypingEnv::fully_monomorphized();
        let Ok(val) = const_.eval(tcx, typing_env, rustc_span::DUMMY_SP) else {
            raise_error!(self, span, "Could not evaluate constant: {const_:?}")
        };
        let src = ConstSource::Value(val, const_.ty());
        self.read_const(span, src, ReadMode::Structured)
    }
}
