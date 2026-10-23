extern crate rustc_abi;
extern crate rustc_apfloat;
extern crate rustc_attr_ir;
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_public;
extern crate rustc_public_bridge;
extern crate rustc_span;

use charon_lib::{ast::*, raise_error, register_error};
use itertools::Itertools;
use log::trace;
use rustc_apfloat::{Float, ieee};
use rustc_const_uneval::{
    self as uneval, AllocTarget, ConstReader, ConstSource, GlobalRef, PtrTarget, ReadError,
    ReadMode,
};
use rustc_middle::mir::interpret::PointerArithmetic;
use rustc_middle::ty as mty;
use rustc_public::rustc_internal::{internal, stable};
use rustc_public::{CrateDef, CrateDefType, abi, mir, ty};
use rustc_public_bridge::IndexedVal;
use std::io::Read;

use crate::translate::translate_ctx::ItemTransCtx;

impl<'tcx, 'ctx> ItemTransCtx<'tcx, 'ctx> {
    /// Utility function used to read an allocation data into a unassigned integer.
    fn read_target_uint(&mut self, mut bytes: &[u8]) -> Result<u128, Error> {
        let mut buf = [0u8; size_of::<u128>()];
        match self.t_ctx.tcx.data_layout.endian {
            rustc_abi::Endian::Little => {
                bytes.read_exact(&mut buf[..bytes.len()])?;
                Ok(u128::from_le_bytes(buf))
            }
            rustc_abi::Endian::Big => {
                bytes.read_exact(&mut buf[16 - bytes.len()..])?;
                Ok(u128::from_be_bytes(buf))
            }
        }
    }

    /// Utility function used to read an allocation data into an assigned integer.
    fn read_target_int(&mut self, mut bytes: &[u8]) -> Result<i128, Error> {
        // we do sign extension manually because why not i guess.
        match self.t_ctx.tcx.data_layout.endian {
            rustc_abi::Endian::Little => {
                let sign = bytes.last().copied().unwrap_or(0) & 0x80 != 0;
                let default = if sign { 0xff } else { 0x00 };
                let mut buf = [default; size_of::<i128>()];
                bytes.read_exact(&mut buf[..bytes.len()])?;
                Ok(i128::from_le_bytes(buf))
            }
            rustc_abi::Endian::Big => {
                let sign = bytes.first().copied().unwrap_or(0) & 0x80 != 0;
                let default = if sign { 0xff } else { 0x00 };
                let mut buf = [default; size_of::<i128>()];
                bytes.read_exact(&mut buf[16 - bytes.len()..])?;
                Ok(i128::from_be_bytes(buf))
            }
        }
    }

    pub fn as_init(bytes: &[Option<u8>]) -> Result<Vec<u8>, Error> {
        bytes
            .iter()
            .copied()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("Found uninitialized bytes when reading {bytes:?}").into())
    }

    /// If a fn pointer to `instance` is a closure `call_once` shim that ignores its closure
    /// argument, return that closure: the pointer then targets the closure-as-fn item.
    fn closure_as_fn_target(
        &self,
        instance: mir::mono::Instance,
        expected_inputs: Option<usize>,
    ) -> Option<(ty::ClosureDef, ty::GenericArgs)> {
        if let Ok(fn_abi) = instance.fn_abi()
            && expected_inputs.is_none_or(|n| fn_abi.args.len() == n + 1)
            && let Some(closure_arg) = fn_abi.args.first()
            && closure_arg.mode == abi::PassMode::Ignore
            && let ty::TyKind::RigidTy(ty::RigidTy::Closure(closure, args)) = closure_arg.ty.kind()
        {
            return Some((closure, args));
        }
        None
    }

    /// Register a fn pointer's target, mapping closure `call_once` shims to the closure-as-fn item.
    fn register_fn_ptr_target(
        &mut self,
        span: Span,
        instance: mir::mono::Instance,
        expected_inputs: Option<usize>,
    ) -> FunDeclId {
        if let Some((closure, args)) = self.closure_as_fn_target(instance, expected_inputs) {
            return self.register_closure_as_fn_id(span, closure, args);
        }
        self.register_fun_decl_id(span, instance)
    }

    /// The type of fn pointers to the target of a fn pointer to `instance` (see
    /// [`Self::register_fn_ptr_target`]).
    fn fn_ptr_target_ty(&mut self, span: Span, instance: mir::mono::Instance) -> Result<Ty, Error> {
        let tcx = self.t_ctx.tcx;
        let sig = if let Some((_, args)) = self.closure_as_fn_target(instance, None) {
            let args = rustc_public::rustc_internal::internal(tcx, args);
            tcx.signature_unclosure(args.as_closure().sig(), rustc_hir::Safety::Safe)
        } else {
            let instance = rustc_public::rustc_internal::internal(tcx, instance);
            let typing_env = rustc_middle::ty::TypingEnv::fully_monomorphized();
            instance.ty(tcx, typing_env).fn_sig(tcx)
        };
        let fn_ptr_ty = rustc_middle::ty::Ty::new_fn_ptr(tcx, sig);
        self.translate_ty(span, rustc_public::rustc_internal::stable(fn_ptr_ty))
    }

    pub fn as_charon_bytes(
        &mut self,
        span: Span,
        alloc: &ty::Allocation,
        offset: usize,
        size: usize,
    ) -> Result<Vec<Byte>, Error> {
        let mut bytes: Vec<Byte> = alloc.bytes.as_slice()[offset..offset + size]
            .iter()
            .map(|b| match b {
                None => Byte::Uninit,
                Some(v) => Byte::Value(*v),
            })
            .collect();

        let ptr_size = self.t_ctx.tcx.pointer_size().bytes_usize();
        for (prov_ofs, prov) in alloc.provenance.ptrs.iter() {
            let prov_alloc: mir::alloc::GlobalAlloc = prov.0.into();
            let prov = match prov_alloc {
                mir::alloc::GlobalAlloc::Function(fun) => {
                    let id = self.register_fn_ptr_target(span, fun, None);
                    let fn_ptr_ty = self.fn_ptr_target_ty(span, fun)?;
                    let fn_ptr = FnPtr {
                        kind: Box::new(FnPtrKind::Fun(id)),
                        generics: Box::new(GenericArgs::empty()),
                    };
                    GlobalProvenance::Function(fn_ptr, fn_ptr_ty)
                }
                _ => {
                    let id = self.register_global_decl_id(span, prov.0, None);
                    GlobalProvenance::Global(GlobalDeclRef {
                        id,
                        generics: Box::new(GenericArgs::empty()),
                    })
                }
            };
            let prov_ofs = *prov_ofs;
            for ptr_i in 0..ptr_size {
                let alloc_i = prov_ofs + ptr_i;
                if offset <= alloc_i && alloc_i < offset + size {
                    bytes[alloc_i] = Byte::Provenance(prov.clone(), ptr_i as u8);
                }
            }
        }

        Ok(bytes)
    }

    fn provenance_at<'a>(
        &mut self,
        alloc: &'a ty::Allocation,
        offset: usize,
    ) -> Option<&'a ty::Prov> {
        alloc
            .provenance
            .ptrs
            .iter()
            .find(|(o, _)| *o == offset)
            .map(|(_, p)| p)
    }

    pub fn translate_allocation(
        &mut self,
        span: Span,
        alloc: &ty::Allocation,
        ty: &TyKind,
        rty: ty::Ty,
    ) -> Result<ConstantExpr, Error> {
        self.translate_allocation_at(span, alloc, ty, rty, 0, None)
    }

    /// Translate a value out of an allocation, starting at byte `offset`.
    ///
    /// `unsized_len` is the number of trailing bytes that make up the unsized data of an unsized
    /// value (`str`, `[T]`, or a struct whose last field is unsized, such as `CStr`). It is only
    /// relevant when `ty` is unsized, since in that case the layout only describes the sized
    /// prefix and the length of the data can't be recovered from the type. For a top-level
    /// unsized constant the caller can pass `None`, in which case we use the rest of the
    /// allocation.
    pub fn translate_allocation_at(
        &mut self,
        span: Span,
        alloc: &ty::Allocation,
        ty: &TyKind,
        rty: ty::Ty,
        offset: usize,
        unsized_len: Option<usize>,
    ) -> Result<ConstantExpr, Error> {
        let layout = rty.layout()?.shape();
        let size = layout.size.bytes();
        // For an unsized value the trailing data extends to the end of the allocation unless the
        // caller told us where it stops (e.g. when it is a field of a larger unsized value).
        let unsized_len = layout
            .is_unsized()
            .then(|| unsized_len.unwrap_or(alloc.bytes.len() - offset));
        if size == 0 && unsized_len.is_none() {
            return self.translate_zst_constant(span, ty, rty);
        }
        let bytes = &alloc.bytes.as_slice()[offset..offset + size];
        let kind = match ty {
            TyKind::Scalar(lit) => match lit {
                ScalarTy::Integer(IntegerTy::Signed(it)) => {
                    ConstantExprKind::Integer(IntegerValue::Signed(
                        it.clone(),
                        self.read_target_int(Self::as_init(bytes)?.as_slice())?,
                    ))
                }
                ScalarTy::Integer(IntegerTy::Unsigned(uit)) => {
                    ConstantExprKind::Integer(IntegerValue::Unsigned(
                        uit.clone(),
                        self.read_target_uint(Self::as_init(bytes)?.as_slice())?,
                    ))
                }
                ScalarTy::Bool => {
                    let bool = self.read_target_int(Self::as_init(bytes)?.as_slice())?;
                    let res = match bool {
                        0 => false,
                        1 => true,
                        _ => {
                            raise_error!(self, span, "Invalid boolean value in constant: {bool}")
                        }
                    };
                    ConstantExprKind::Bool(res)
                }
                ScalarTy::Char => ConstantExprKind::Char(
                    char::from_u32(self.read_target_uint(Self::as_init(bytes)?.as_slice())? as u32)
                        .unwrap(),
                ),
                ScalarTy::Float(f) => {
                    let bits = self.read_target_uint(Self::as_init(bytes)?.as_slice())?;
                    let value = match f {
                        FloatTy::F16 => ieee::Half::from_bits(bits).to_string(),
                        FloatTy::F32 => ieee::Single::from_bits(bits).to_string(),
                        FloatTy::F64 => ieee::Double::from_bits(bits).to_string(),
                        FloatTy::F128 => ieee::Quad::from_bits(bits).to_string(),
                    };
                    ConstantExprKind::Float(FloatValue {
                        value,
                        ty: f.clone(),
                    })
                }
            },
            TyKind::Ref(_, subty, _) | TyKind::RawPtr(subty, _) => 'ptr_case: {
                let Some(suballoc) = self.provenance_at(alloc, offset) else {
                    let value = self.read_target_uint(Self::as_init(bytes)?.as_slice())?;
                    break 'ptr_case ConstantExprKind::PtrNoProvenance(value);
                };
                use mir::alloc::GlobalAlloc;
                let alloc_id = suballoc.0;
                let glob_alloc: GlobalAlloc = alloc_id.into();
                match glob_alloc {
                    GlobalAlloc::Memory(suballoc) if subty.is_str() => {
                        let as_str =
                            unsafe { String::from_utf8_unchecked(suballoc.raw_bytes().unwrap()) };
                        ConstantExprKind::Str(as_str)
                    }
                    GlobalAlloc::Memory(suballoc) if subty.is_slice() => {
                        let meta_bytes = &alloc.bytes.as_slice()[offset + size / 2..offset + size];
                        let len = self.read_target_uint(Self::as_init(meta_bytes)?.as_slice())?;
                        let subty = subty.as_array_or_slice().unwrap();
                        let rtyk = rty.kind().builtin_deref(true).unwrap().ty.kind();
                        let ty::RigidTy::Slice(rsubty) = rtyk.rigid().unwrap() else {
                            unreachable!("Unexpected rigid type for slice: {rty:?}");
                        };
                        let elem_size = rsubty.layout().unwrap().shape().size.bytes();
                        let sub_constants = (0..len as usize)
                            .map(|i| {
                                let elem_off = elem_size * i;
                                self.translate_allocation_at(
                                    span,
                                    &suballoc,
                                    subty.kind(),
                                    *rsubty,
                                    elem_off,
                                    None,
                                )
                            })
                            .try_collect()?;
                        let len = ConstantExpr::mk_usize(len);
                        let sub_constant = ConstantExpr::new(
                            ConstantExprKind::Array(sub_constants),
                            Ty::mk_array(subty.clone(), len.clone(), None),
                        );
                        let place = GlobalPlace::new_value(sub_constant);
                        let metadata = Some(UnsizingMetadata::Length(len));

                        if let TyKind::RawPtr(_, rk) = ty {
                            ConstantExprKind::Ptr(*rk, place, metadata)
                        } else {
                            ConstantExprKind::Ref(place, metadata)
                        }
                    }
                    // The pointer points directly at a function (e.g. `f as *mut ()`
                    GlobalAlloc::Function(instance) => {
                        let id = self.register_fn_ptr_target(span, instance, None);
                        let generics = self.translate_generic_args(span, &instance.args())?;
                        ConstantExprKind::FnPtr(FnPtr {
                            generics: Box::new(generics),
                            kind: Box::new(FnPtrKind::Fun(id)),
                        })
                    }
                    _ => {
                        let pointee_has_len_metadata = || {
                            let tcx = self.t_ctx.tcx;
                            let Some(pointee) = rty.kind().builtin_deref(true).map(|d| d.ty) else {
                                return false;
                            };
                            let pointee: rustc_middle::ty::Ty<'_> =
                                rustc_public::rustc_internal::internal(tcx, pointee);
                            let typing_env = rustc_middle::ty::TypingEnv::fully_monomorphized();
                            matches!(
                                tcx.struct_tail_for_codegen(pointee, typing_env).kind(),
                                rustc_middle::ty::Str | rustc_middle::ty::Slice(..)
                            )
                        };
                        let (metadata, dyn_self_ty) = match subty.kind() {
                            // Slices, and (e.g. `CStr`) structs with a slice or `str` tail.
                            TyKind::Slice(..) | TyKind::Adt(..) if pointee_has_len_metadata() => {
                                let meta_bytes =
                                    &alloc.bytes.as_slice()[offset + size / 2..offset + size];
                                let len =
                                    self.read_target_uint(Self::as_init(meta_bytes)?.as_slice())?;
                                let meta = UnsizingMetadata::Length(
                                    IntegerValue::Unsigned(UIntTy::Usize, len).to_constant(),
                                );
                                (Some(meta), None)
                            }
                            TyKind::DynTrait(_) => {
                                let Some(vtable_prov) =
                                    self.provenance_at(alloc, offset + size / 2)
                                else {
                                    unreachable!("&dyn constant with no vtable provenance?");
                                };
                                let alloc: mir::alloc::GlobalAlloc = vtable_prov.0.into();
                                let mir::alloc::GlobalAlloc::VTable(self_ty, trait_ref) = alloc
                                else {
                                    unreachable!("&dyn constant with non-vtable provenance?");
                                };
                                let trait_ref = trait_ref.map(|t| {
                                    let t =
                                        rustc_public::rustc_internal::internal(self.t_ctx.tcx, t);
                                    let t = self.t_ctx.tcx.instantiate_bound_regions_with_erased(t);
                                    let t = rustc_public::rustc_internal::stable(t);
                                    t.with_self_ty(self_ty)
                                });
                                let meta =
                                    self.translate_vtable_ptr_const(span, self_ty, trait_ref);
                                let meta = UnsizingMetadata::VTable(self.dummy_trait_ref(), meta);
                                (Some(meta), Some(self_ty))
                            }
                            _ => (None, None),
                        };

                        let glob_ty = match (&glob_alloc, dyn_self_ty) {
                            // we try checking if there's a static we can use the type of,
                            // to avoid registering unsized globals (a static is always sized)
                            (GlobalAlloc::Static(stt), _) => stt.ty(),
                            // for a `&dyn Trait` the pointee type is unsized; use the concrete
                            // type recovered from the vtable so we register a sized global.
                            (_, Some(dyn_self_ty)) => dyn_self_ty,
                            // if we don't know the exact type, we create a dummy type
                            // that should match
                            (GlobalAlloc::Memory(mem), _)
                                if let Some(inner) =
                                    rty.kind().builtin_deref(true).map(|d| d.ty)
                                    && let Ok(layout) = inner.layout()
                                    && !layout.shape().is_unsized()
                                    && mem.bytes.len() > layout.shape().size.bytes() as usize =>
                            {
                                self.raw_alloc_ty(mem.bytes.len(), mem.align as usize)?
                            }
                            _ => rty.kind().builtin_deref(true).unwrap().ty,
                        };

                        let id = self.register_global_decl_id(span, alloc_id, Some(glob_ty));
                        let glob_ty = self.translate_ty(span, glob_ty)?;
                        let generics = match glob_alloc {
                            GlobalAlloc::Static(stt) => {
                                let instance: mir::mono::Instance = stt.into();
                                self.translate_generic_args(span, &instance.args())?
                            }
                            _ => GenericArgs::empty(),
                        };
                        let place = GlobalPlace::new_global(
                            GlobalDeclRef {
                                id,
                                generics: Box::new(generics),
                            },
                            glob_ty,
                        );

                        if let TyKind::RawPtr(_, rk) = ty {
                            ConstantExprKind::Ptr(*rk, place, metadata)
                        } else {
                            ConstantExprKind::Ref(place, metadata)
                        }
                    }
                }
            }
            TyKind::Adt(tref) if tref.is_tuple() => {
                let rtyk = rty.kind();
                let ty::RigidTy::Tuple(rtys) = rtyk.rigid().unwrap() else {
                    unreachable!("Unexpected rigid type for tuple: {rty:?}");
                };
                let layout = rty.layout()?.shape();
                let abi::FieldsShape::Arbitrary { offsets } = &layout.fields else {
                    unreachable!("Unexpected layout for tuple: {layout:?}");
                };
                let fields = (0..rtys.len())
                    .map(|i| {
                        let field_offset = offsets[i].bytes() as usize;
                        let field_rty = rtys[i];
                        let field_ty = self.translate_ty(span, field_rty)?;
                        // Only the last field of an unsized tuple is itself unsized.
                        let field_len = unsized_len
                            .filter(|_| i + 1 == rtys.len())
                            .map(|l| l - field_offset);
                        self.translate_allocation_at(
                            span,
                            alloc,
                            field_ty.kind(),
                            field_rty,
                            field_offset + offset,
                            field_len,
                        )
                    })
                    .try_collect()?;
                ConstantExprKind::Adt(None, fields)
            }
            TyKind::Adt(tref) if tref.builtin.is_none() => {
                let rtyk = rty.kind();
                let ty::RigidTy::Adt(adt, generics) = rtyk.rigid().unwrap() else {
                    unreachable!("Unexpected rigid type for adt: {rty:?}");
                };
                let layout = rty.layout()?.shape();
                let (variant, rfields, offsets) = match adt.kind() {
                    ty::AdtKind::Struct => {
                        let variant = adt.variants()[0];
                        let rfields = variant
                            .fields()
                            .iter()
                            .map(|f| f.ty_with_args(generics))
                            .collect::<Vec<ty::Ty>>();
                        let abi::FieldsShape::Arbitrary { offsets } = &layout.fields else {
                            unreachable!("Unexpected layout for struct: {layout:?}");
                        };
                        (None, rfields, offsets.clone())
                    }
                    ty::AdtKind::Enum => 'enum_case: {
                        if let abi::VariantsShape::Single { index } = layout.variants {
                            let variant = VariantId::new(index.to_index());
                            let variant_data = adt.variant(index).unwrap();
                            let rfields = variant_data
                                .fields()
                                .iter()
                                .map(|f| f.ty_with_args(generics))
                                .collect::<Vec<ty::Ty>>();
                            let abi::FieldsShape::Arbitrary { offsets } = &layout.fields else {
                                unreachable!("Unexpected layout for struct: {layout:?}");
                            };
                            break 'enum_case (Some(variant), rfields, offsets.clone());
                        };
                        let abi::VariantsShape::Multiple {
                            tag,
                            tag_encoding,
                            tag_field,
                            variants,
                        } = &layout.variants
                        else {
                            unreachable!("Unexpected layout for enum: {layout:?}");
                        };
                        assert!(layout.fields.count() == 1, "Enum with non-1 shared field?");
                        let abi::FieldsShape::Arbitrary { offsets } = &layout.fields else {
                            unreachable!("Unexpected layout for enum: {layout:?}");
                        };
                        let abi::Scalar::Initialized { value: tag_ty, .. } = &tag else {
                            unreachable!("Unexpected tag encoding for enum: {tag:?}");
                        };
                        let (length_bytes, _) = match tag_ty {
                            abi::Primitive::Int { length, signed } => (length.bits() / 8, *signed),
                            abi::Primitive::Pointer(_) => (
                                self.t_ctx.tcx.data_layout.pointer_size().bytes() as usize,
                                false,
                            ),
                            abi::Primitive::Float { .. } => unreachable!("Float tag?"),
                        };
                        let tag_offset = offsets[*tag_field].bytes();
                        let tag_bytes = &bytes[tag_offset..tag_offset + length_bytes];
                        let tag_value =
                            self.read_target_uint(Self::as_init(tag_bytes)?.as_slice())?;
                        let mask = if length_bytes * 8 == 128 {
                            u128::MAX
                        } else {
                            (1u128 << (length_bytes * 8)) - 1
                        };
                        let variant_idx = match tag_encoding {
                            abi::TagEncoding::Direct => (0..adt.num_variants())
                                .find_map(|idx| {
                                    let idx = ty::VariantIdx::to_val(idx);
                                    let discr = adt.discriminant_for_variant(idx);
                                    ((discr.val & mask) == tag_value).then_some(idx)
                                })
                                .unwrap(),
                            abi::TagEncoding::Niche {
                                untagged_variant,
                                niche_variants,
                                niche_start,
                            } => (0..adt.num_variants())
                                .find_map(|idx| {
                                    let idx = ty::VariantIdx::to_val(idx);
                                    let discr = adt.discriminant_for_variant(idx);
                                    let niche_variants_start =
                                        niche_variants.start().to_index() as u128;

                                    if matches!(tag_ty, abi::Primitive::Int { .. }) {
                                        if discr.val < niche_variants_start {
                                            // The variant is before the niche variants, so it is not tagged.
                                            // Could e.g. be the case if it is uninhabited.
                                            return None;
                                        }
                                        let tag = (discr.val - niche_variants_start)
                                            .wrapping_add(*niche_start);
                                        (tag_value == tag).then_some(idx)
                                    } else {
                                        // pointer niche: if no provenance, then niche variant,
                                        // otherwise untagged variant
                                        assert!(
                                            niche_variants.start() == niche_variants.end(),
                                            ">1 niche in ptr niche?"
                                        );
                                        self.provenance_at(alloc, offset)
                                            .is_none()
                                            .then_some(*niche_variants.start())
                                    }
                                })
                                .unwrap_or(*untagged_variant),
                        };
                        let variant = adt.variant(variant_idx).unwrap();
                        let fields = variant
                            .fields()
                            .iter()
                            .map(|f| f.ty_with_args(generics))
                            .collect::<Vec<ty::Ty>>();
                        let variant_idx_charon = self.translate_variant_id(variant_idx);
                        let variant_layout = variants.get(variant_idx.to_index()).unwrap();
                        (
                            Some(variant_idx_charon),
                            fields,
                            variant_layout.offsets.clone(),
                        )
                    }
                    ty::AdtKind::Union => {
                        return Ok(ConstantExpr::new(
                            ConstantExprKind::RawMemory(
                                self.as_charon_bytes(span, alloc, offset, size)?,
                            ),
                            ty.clone().into_ty(),
                        ));
                    }
                };

                let consts = (0..offsets.len())
                    .map(|field| {
                        let field_offset = offsets[field].bytes() as usize;
                        let field_rty = rfields[field];
                        let field_ty = self.translate_ty(span, field_rty)?;
                        // Only an unsized struct's last field is itself unsized; enums are
                        // always sized so `unsized_len` is `None` for them.
                        let field_len = unsized_len
                            .filter(|_| field + 1 == offsets.len())
                            .map(|l| l - field_offset);
                        self.translate_allocation_at(
                            span,
                            alloc,
                            field_ty.kind(),
                            field_rty,
                            field_offset + offset,
                            field_len,
                        )
                    })
                    .try_collect()?;
                ConstantExprKind::Adt(variant.clone(), consts)
            }
            TyKind::Array(subty, ..) => {
                let rtyk = rty.kind();
                let ty::RigidTy::Array(subrty, _) = rtyk.rigid().unwrap() else {
                    unreachable!();
                };
                let layout = rty.layout()?.shape();
                let abi::FieldsShape::Array { stride, count } = layout.fields else {
                    unreachable!("Unexpected layout for array: {layout:?}");
                };
                let stride = stride.bytes();
                let elems = (0..count as usize)
                    .map(|i| {
                        let elem_off = stride * i + offset;
                        self.translate_allocation_at(span, alloc, subty, *subrty, elem_off, None)
                    })
                    .try_collect()?;
                ConstantExprKind::Array(elems)
            }
            TyKind::FnPtr(_) => 'fnptr_case: {
                let Some(prov) = self.provenance_at(alloc, offset) else {
                    let value = self.read_target_uint(Self::as_init(bytes)?.as_slice())?;
                    break 'fnptr_case ConstantExprKind::PtrNoProvenance(value);
                };
                use mir::alloc::GlobalAlloc;
                let glob_alloc: GlobalAlloc = prov.0.into();
                match glob_alloc {
                    GlobalAlloc::Function(instance) => {
                        let TyKind::FnPtr(sig) = ty else {
                            unreachable!("Unexpected type for fn ptr: {ty:?}");
                        };

                        // special-case: if the first argument is ignored, because the signature
                        // is shorter, we do a closure_as_fn conversion
                        let id = self.register_fn_ptr_target(
                            span,
                            instance,
                            Some(sig.skip_binder.inputs.len()),
                        );

                        let generics = self.translate_generic_args(span, &instance.args())?;
                        let fn_ptr = FnPtr {
                            generics: Box::new(generics),
                            kind: Box::new(FnPtrKind::Fun(id)),
                        };
                        ConstantExprKind::FnPtr(fn_ptr)
                    }
                    _ => {
                        trace!("Gave up for raw memory of fndef with alloc {glob_alloc:?}");
                        ConstantExprKind::RawMemory(
                            self.as_charon_bytes(span, alloc, offset, size)?,
                        )
                    }
                }
            }
            TyKind::Pattern(inner, _) => {
                let rtyk = rty.kind();
                let Some(ty::RigidTy::Pat(rty, _)) = rtyk.rigid() else {
                    unreachable!("Pattern type should be a rigid pattern type");
                };
                self.translate_allocation_at(span, alloc, inner, *rty, offset, unsized_len)?
                    .kind()
                    .clone()
            }

            TyKind::Adt(tref) if tref.is_box() => {
                unreachable!("We never create builtin boxes");
            }
            // An unsized `str` held directly in a constant: its data is the raw UTF-8 bytes.
            TyKind::Adt(tref) if tref.is_str() => {
                let len = unsized_len.expect("str constant without a length");
                let data = &alloc.bytes.as_slice()[offset..offset + len];
                let as_str = unsafe { String::from_utf8_unchecked(Self::as_init(data)?) };
                ConstantExprKind::Str(as_str)
            }
            // An unsized `[T]` held directly in a constant.
            TyKind::Slice(subty, _) => {
                let len = unsized_len.expect("slice constant without a length");
                let rtyk = rty.kind();
                let ty::RigidTy::Slice(subrty) = rtyk.rigid().unwrap() else {
                    unreachable!("Unexpected rigid type for slice: {rty:?}");
                };
                let elem_size = subrty.layout()?.shape().size.bytes() as usize;
                let count = if elem_size == 0 { 0 } else { len / elem_size };
                let elems = (0..count)
                    .map(|i| {
                        let elem_off = offset + elem_size * i;
                        self.translate_allocation_at(span, alloc, subty, *subrty, elem_off, None)
                    })
                    .try_collect()?;
                ConstantExprKind::Array(elems)
            }
            // A bare `dyn Trait` value can't be held by-value in a constant.
            TyKind::DynTrait(..) => {
                unreachable!("Translating unsized dyn constant?");
            }
            TyKind::Error(..)
            | TyKind::Never
            | TyKind::TypeVar(..)
            | TyKind::TraitType(..)
            | TyKind::FnDef(..)
            | TyKind::PtrMetadata(..) => {
                trace!("Gave up for raw memory of type {ty:?} with alloc {alloc:?}");
                ConstantExprKind::RawMemory(self.as_charon_bytes(span, alloc, offset, size)?)
            }
            TyKind::Adt(tref) => {
                unreachable!("Unhandled builtin type in constant: {:?}", tref.builtin);
            }
        };
        Ok(ConstantExpr::new(kind, ty.clone().into_ty()))
    }

    pub fn translate_zst_constant(
        &mut self,
        span: Span,
        ty: &TyKind,
        rty: ty::Ty,
    ) -> Result<ConstantExpr, Error> {
        let kind = match ty {
            TyKind::FnDef(fnptr) => ConstantExprKind::FnDef(fnptr.skip_binder.clone()),
            TyKind::Array(subty, ..) => {
                let rtyk = rty.kind();
                let ty::RigidTy::Array(rty, len) = rtyk.rigid().unwrap() else {
                    unreachable!();
                };
                let len = len.eval_target_usize()?;
                if len > 32 {
                    // FIXME: temporary safeguard for large arrays; ideally we should have some
                    // sort of ConstantExprKind::ArrayRepeat...
                    ConstantExprKind::RawMemory(vec![])
                } else if len == 0 {
                    ConstantExprKind::Array(vec![])
                } else {
                    let cexpr = self.translate_zst_constant(span, subty, *rty)?;
                    ConstantExprKind::Array(vec![cexpr; len as usize])
                }
            }
            TyKind::Adt(tref) if tref.builtin.is_none() || tref.is_tuple() => {
                let rtyk = rty.kind();
                let (variant, rtys) = match rtyk.rigid().unwrap() {
                    ty::RigidTy::Tuple(rtys) => (None, rtys.clone()),
                    ty::RigidTy::Closure(_, generics) => {
                        let tupled_upvars = generics.0.last().unwrap().expect_ty().kind();
                        let Some(ty::RigidTy::Tuple(state_tys)) = tupled_upvars.rigid() else {
                            raise_error!(self, span, "Closure state argument is not a tuple?");
                        };
                        (None, state_tys.clone())
                    }
                    ty::RigidTy::Adt(adt, generics) => {
                        let layout = rty.layout()?.shape();
                        let variant_r = match adt.kind() {
                            ty::AdtKind::Struct => None,
                            ty::AdtKind::Enum => {
                                let abi::VariantsShape::Single { index } = layout.variants else {
                                    trace!(
                                        "Unexpected layout for ZST enum\n- Layout: {layout:?}\n- Ty: {rty:?}"
                                    );
                                    return Ok(ConstantExpr::new(
                                        ConstantExprKind::RawMemory(vec![]),
                                        ty.clone().into_ty(),
                                    ));
                                };
                                Some(index)
                            }
                            ty::AdtKind::Union => {
                                return Ok(ConstantExpr::new(
                                    ConstantExprKind::RawMemory(vec![]),
                                    ty.clone().into_ty(),
                                ));
                            }
                        };
                        let variant = variant_r.map(|v| self.translate_variant_id(v));
                        let variant_r = variant_r.unwrap_or(ty::VariantIdx::to_val(0));
                        let fields = adt
                            .variant(variant_r)
                            .unwrap()
                            .fields()
                            .iter()
                            .map(|f| f.ty_with_args(generics))
                            .collect();
                        (variant, fields)
                    }
                    _ => unreachable!("Unexpected rigid type for adt: {rty:?} with kind {rtyk:?}"),
                };
                let fields = rtys
                    .into_iter()
                    .map(|field_rty| {
                        let field_ty = self.translate_ty(span, field_rty)?;
                        self.translate_zst_constant(span, field_ty.kind(), field_rty)
                    })
                    .try_collect()?;
                ConstantExprKind::Adt(variant, fields)
            }
            _ => {
                raise_error!(self, span, "Unsupported ZST constant type: {:?}", ty)
            }
        };
        Ok(ConstantExpr::new(kind, ty.clone().into_ty()))
    }

    pub fn translate_tyconst_to_const_expr(
        &mut self,
        span: Span,
        v: &ty::TyConst,
    ) -> Result<ConstantExpr, Error> {
        match v.kind() {
            ty::TyConstKind::Value(rty, alloc) => {
                let ty = self.translate_ty(span, *rty)?;
                self.translate_allocation(span, alloc, ty.kind(), *rty)
            }
            ty::TyConstKind::ZSTValue(rty) => {
                let ty = self.translate_ty(span, *rty)?;
                self.translate_zst_constant(span, ty.kind(), *rty)
            }
            ty::TyConstKind::Unevaluated(..) => {
                // An unevaluated const (e.g. an array length or const-generic argument that
                // refers to another `const` item). We evaluate it by normalizing the internal
                // `ty::Const` (which triggers const evaluation), then translate the resulting
                // value.
                let internal = rustc_public::rustc_internal::internal(self.t_ctx.tcx, v);
                let typing_env = rustc_middle::ty::TypingEnv::fully_monomorphized();
                // We use the `try_` variant since `normalize_erasing_regions` ICEs if
                // normalization fails (e.g. when the const is too generic to evaluate). obol
                // only ever sees monomorphic code, so this should always succeed in practice,
                // but we degrade to a translation error rather than crashing if it doesn't.
                let Ok(normalized) = self.t_ctx.tcx.try_normalize_erasing_regions(
                    typing_env,
                    rustc_middle::ty::Unnormalized::new_wip(internal),
                ) else {
                    raise_error!(self, span, "Could not evaluate constant: {:?}", v.kind())
                };
                let evaluated = rustc_public::rustc_internal::stable(normalized);
                if matches!(evaluated.kind(), ty::TyConstKind::Unevaluated(..)) {
                    // Normalization made no progress; bail out to avoid looping.
                    raise_error!(self, span, "Could not evaluate constant: {:?}", v.kind())
                }
                self.translate_tyconst_to_const_expr(span, &evaluated)
            }
            ty::TyConstKind::Param(param) => {
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
                Ok(ConstantExpr::new(
                    ConstantExprKind::Var(ConstGenericDbVar::Free(id)),
                    ty,
                ))
            }
            _ => {
                raise_error!(
                    self,
                    span,
                    "Unexpected constant expression kind: {:?}",
                    v.kind()
                )
            }
        }
    }

    /// The reader for constants; we only see monomorphic ones.
    fn const_reader(&self) -> ConstReader<'tcx> {
        ConstReader {
            tcx: self.t_ctx.tcx,
            typing_env: mty::TypingEnv::fully_monomorphized(),
            config: uneval::Config {
                anon_allocs_as_globals: true,
            },
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
                    Some(metadata) => CastKind::Unsize(ty.clone(), metadata),
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
                        uneval::Byte::Ptr(target, i) => {
                            if last_prov.as_ref().is_none_or(|(last, _)| *last != target) {
                                let prov = self.translate_alloc_target(span, target)?;
                                last_prov = Some((target, prov));
                            }
                            Byte::Provenance(last_prov.as_ref().unwrap().1.clone(), i)
                        }
                    });
                }
                ConstantExprKind::RawMemory(charon_bytes)
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
        let metadata =
            self.translate_unsizing_metadata(span, ref_to(unsize.from), ref_to(unsize.to))?;
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
            AllocTarget::Global(global) if self.const_reader().is_named_global(global) => {
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
            // The other globals (nested statics) aren't translated, and we don't translate the
            // pointers of a `TypeId` (see `translate_type_id_const`).
            AllocTarget::Global(_) | AllocTarget::TypeId(_) => {
                return Ok(GlobalProvenance::Unknown);
            }
        };
        Ok(GlobalProvenance::Global(GlobalDeclRef {
            id,
            generics: Box::new(GenericArgs::empty()),
        }))
    }

    /// A `TypeId` is made of pointers into a zero-sized allocation that stands for its type, which
    /// can't be read back. We instead recognize such constants as the `TypeId` of that type.
    fn translate_type_id_const(
        &mut self,
        span: Span,
        const_: &ty::MirConst,
    ) -> Result<Option<ConstantExpr>, Error> {
        let tcx = self.t_ctx.tcx;
        let is_type_id = internal(tcx, const_.ty())
            .ty_adt_def()
            .is_some_and(|adt_def| {
                tcx.is_lang_item(adt_def.did(), rustc_attr_ir::lang_items::LangItem::TypeId)
            });
        if !is_type_id {
            return Ok(None);
        }
        let ty::ConstantKind::Allocated(alloc) = const_.kind() else {
            return Ok(None);
        };
        let Some((_, prov)) = alloc.provenance.ptrs.first() else {
            return Ok(None);
        };
        let mir::alloc::GlobalAlloc::TypeId { ty } = prov.0.into() else {
            return Ok(None);
        };
        let ty = self.translate_ty(span, ty)?;
        let type_id_ty = self.translate_ty(span, const_.ty())?;
        Ok(Some(ConstantExpr::new(
            ConstantExprKind::TypeId(ty),
            type_id_ty,
        )))
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
        if let Some(type_id) = self.translate_type_id_const(span, const_)? {
            return Ok(type_id);
        }
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
