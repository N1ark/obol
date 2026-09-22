//! Conversions from rustc's own enums to the copies charon keeps in
//! `charon_lib::ast::from_rustc`.

extern crate rustc_ast;
extern crate rustc_attr_ir;
extern crate rustc_span;

use charon_lib::ast::from_rustc::{self, FromRustcError};
use rustc_ast::attr::version::RustcVersion;
use rustc_attr_ir::lang_items::LangItem;
use rustc_hir::attrs::{AttributeKind, DeprecatedSince, Deprecation, InlineAttr, OptimizeAttr};
use rustc_span::symbol::Ident;

use crate::translate::translate_ctx::TranslateCtx;

/// Charon generates its `LangItem` as a verbatim copy of rustc's, so the two agree variant for
/// variant and the mapping is the identity. Listing the variants (rather than transmuting) keeps
/// the match exhaustive, so a rustc bump that adds or renames a lang item fails to compile here
/// instead of silently mistranslating. Regenerate the list from charon's
/// `charon-driver/translate/translate_from_rustc.rs` when bumping charon.
macro_rules! translate_lang_items {
    ($($variant:ident),* $(,)?) => {
        pub(crate) fn translate_lang_item(value: LangItem) -> from_rustc::LangItem {
            match value {
                $(LangItem::$variant => from_rustc::LangItem::$variant,)*
            }
        }
    };
}

translate_lang_items!(
    Sized,
    MetaSized,
    PointeeSized,
    Unsize,
    AlignOf,
    SizeOf,
    OffsetOf,
    StructuralPeq,
    Copy,
    Clone,
    CloneFn,
    UseCloned,
    TrivialClone,
    Sync,
    DiscriminantKind,
    Discriminant,
    PointeeTrait,
    Metadata,
    DynMetadata,
    NonNull,
    Freeze,
    UnsafeUnpin,
    FnPtrTrait,
    FnPtrAsPtr,
    FnPtrFromPtr,
    Code,
    Drop,
    Destruct,
    AsyncDrop,
    AsyncDropInPlace,
    CoerceUnsized,
    DispatchFromDyn,
    TryAsDyn,
    TransmuteOpts,
    TransmuteTrait,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Neg,
    Not,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    RemAssign,
    BitXorAssign,
    BitAndAssign,
    BitOrAssign,
    ShlAssign,
    ShrAssign,
    Index,
    IndexMut,
    UnsafeCell,
    CovariantUnsafeCell,
    UnsafePinned,
    VaArgSafe,
    VaList,
    Complex,
    Deref,
    DerefMut,
    DerefPure,
    DerefTarget,
    Receiver,
    ReceiverTarget,
    LegacyReceiver,
    Fn,
    FnMut,
    FnOnce,
    FnStatic,
    AsyncFn,
    AsyncFnMut,
    AsyncFnOnce,
    AsyncFnOnceOutput,
    CallOnceFuture,
    CallRefFuture,
    AsyncFnKindHelper,
    AsyncFnKindUpvars,
    FnOnceOutput,
    Iterator,
    FusedIterator,
    Future,
    FutureOutput,
    AsyncIterator,
    CoroutineState,
    Coroutine,
    CoroutineReturn,
    CoroutineYield,
    CoroutineResume,
    Unpin,
    Pin,
    OrderingEnum,
    PartialEq,
    PartialOrd,
    CVoid,
    Type,
    TypeGeneric,
    TypeId,
    Panic,
    PanicNounwind,
    PanicFmt,
    PanicDisplay,
    ConstPanicFmt,
    PanicBoundsCheck,
    PanicMisalignedPointerDereference,
    PanicInfo,
    PanicLocation,
    PanicImpl,
    PanicCannotUnwind,
    PanicInCleanup,
    PanicAddOverflow,
    PanicSubOverflow,
    PanicMulOverflow,
    PanicDivOverflow,
    PanicRemOverflow,
    PanicNegOverflow,
    PanicShrOverflow,
    PanicShlOverflow,
    PanicDivZero,
    PanicRemZero,
    PanicCoroutineResumed,
    PanicAsyncFnResumed,
    PanicAsyncGenFnResumed,
    PanicGenFnNone,
    PanicCoroutineResumedPanic,
    PanicAsyncFnResumedPanic,
    PanicAsyncGenFnResumedPanic,
    PanicGenFnNonePanic,
    PanicNullPointerDereference,
    PanicNullReferenceConstructed,
    PanicInvalidEnumConstruction,
    PanicCoroutineResumedDrop,
    PanicAsyncFnResumedDrop,
    PanicAsyncGenFnResumedDrop,
    PanicGenFnNoneDrop,
    BeginPanic,
    FormatArgument,
    FormatArguments,
    DropGlue,
    AllocLayout,
    Start,
    EhPersonality,
    CompilerMove,
    CompilerCopy,
    OwnedBox,
    GlobalAlloc,
    PhantomData,
    ManuallyDrop,
    MaybeDangling,
    BikeshedGuaranteedNoDrop,
    MaybeUninit,
    Termination,
    Try,
    Tuple,
    SliceLen,
    TryTraitFromResidual,
    TryTraitFromOutput,
    TryTraitBranch,
    TryTraitFromYeet,
    ResidualIntoTryType,
    CoercePointeeValidated,
    ConstParamTy,
    Poll,
    PollReady,
    PollPending,
    AsyncGenReady,
    AsyncGenPending,
    AsyncGenFinished,
    ResumeTy,
    GetContext,
    Context,
    FuturePoll,
    AsyncIteratorPollNext,
    IntoAsyncIterIntoIter,
    Option,
    OptionSome,
    OptionNone,
    ResultOk,
    ResultErr,
    ControlFlowContinue,
    ControlFlowBreak,
    IntoFutureIntoFuture,
    IntoIterIntoIter,
    IteratorNext,
    PinNewUnchecked,
    RangeFrom,
    RangeFull,
    RangeInclusiveStruct,
    RangeInclusiveNew,
    Range,
    RangeToInclusive,
    RangeTo,
    RangeMax,
    RangeMin,
    RangeSub,
    RangeFromCopy,
    RangeCopy,
    RangeInclusiveCopy,
    RangeToInclusiveCopy,
    String,
    CStr,
    ContractBuildCheckEnsures,
    ContractCheckRequires,
    DefaultTrait4,
    DefaultTrait3,
    DefaultTrait2,
    DefaultTrait1,
    ContractCheckEnsures,
    Reborrow,
    CoerceShared,
    FieldRepresentingType,
    Field,
    FieldBase,
    FieldType,
    FieldOffset,
    From,
    FromFn,
    FnPtr,
);

/// Charon models only a subset of rustc's *parsed* attributes as `from_rustc::AttributeKind`;
/// attributes outside it map to `Err(FromRustcError)` and are dropped by the caller. This mirrors
/// charon's generated `charon-driver/translate/translate_from_rustc.rs`, with rustc spans
/// translated through `translate_span_from_rustc` rather than hax's; regenerate it from there when
/// bumping charon. Unlike charon's generated copy, the auxiliary enums below are matched
/// exhaustively, so a rustc bump that adds a variant fails to compile here rather than silently
/// dropping the attribute.
impl TranslateCtx<'_> {
    pub(crate) fn translate_rustc_attribute_kind(
        &mut self,
        value: &AttributeKind,
    ) -> Result<from_rustc::AttributeKind, FromRustcError> {
        self.translate_attribute_kind_from_rustc(value)
    }

    fn translate_attribute_kind_from_rustc(
        &mut self,
        value: &AttributeKind,
    ) -> Result<from_rustc::AttributeKind, FromRustcError> {
        match value {
            AttributeKind::AutomaticallyDerived => {
                Ok(from_rustc::AttributeKind::AutomaticallyDerived)
            }
            AttributeKind::Cold => Ok(from_rustc::AttributeKind::Cold),
            AttributeKind::Deprecated { deprecation, span } => {
                Ok(from_rustc::AttributeKind::Deprecated {
                    deprecation: self.translate_deprecation_from_rustc(deprecation)?,
                    span: self.translate_span_from_rustc(span),
                })
            }
            AttributeKind::Fundamental => Ok(from_rustc::AttributeKind::Fundamental),
            AttributeKind::Ignore { span, reason } => Ok(from_rustc::AttributeKind::Ignore {
                span: self.translate_span_from_rustc(span),
                reason: (reason)
                    .as_ref()
                    .map(|value| Ok((value).to_string().into()))
                    .transpose()?,
            }),
            AttributeKind::Inline(_0, _1) => Ok(from_rustc::AttributeKind::Inline(
                self.translate_inline_attr_from_rustc(_0)?,
                self.translate_span_from_rustc(_1),
            )),
            AttributeKind::MayDangle(_0) => Ok(from_rustc::AttributeKind::MayDangle(
                self.translate_span_from_rustc(_0),
            )),
            AttributeKind::Naked(_0) => Ok(from_rustc::AttributeKind::Naked(
                self.translate_span_from_rustc(_0),
            )),
            AttributeKind::NoLink => Ok(from_rustc::AttributeKind::NoLink),
            AttributeKind::NoMangle(_0) => Ok(from_rustc::AttributeKind::NoMangle(
                self.translate_span_from_rustc(_0),
            )),
            AttributeKind::NonExhaustive(_0) => Ok(from_rustc::AttributeKind::NonExhaustive(
                self.translate_span_from_rustc(_0),
            )),
            AttributeKind::Optimize(_0, _1) => Ok(from_rustc::AttributeKind::Optimize(
                self.translate_optimize_attr_from_rustc(_0)?,
                self.translate_span_from_rustc(_1),
            )),
            AttributeKind::RustcAlign { align, span } => {
                Ok(from_rustc::AttributeKind::RustcAlign {
                    align: align.bytes(),
                    span: self.translate_span_from_rustc(span),
                })
            }
            AttributeKind::RustcIntrinsic => Ok(from_rustc::AttributeKind::RustcIntrinsic),
            AttributeKind::ShouldPanic { reason } => Ok(from_rustc::AttributeKind::ShouldPanic {
                reason: (reason)
                    .as_ref()
                    .map(|value| Ok((value).to_string().into()))
                    .transpose()?,
            }),
            AttributeKind::TargetFeature {
                features,
                attr_span,
                was_forced,
            } => Ok(from_rustc::AttributeKind::TargetFeature {
                features: (features)
                    .iter()
                    .map(|value| {
                        Ok((
                            (&((value).0)).to_string().into(),
                            self.translate_span_from_rustc(&((value).1)),
                        ))
                    })
                    .collect::<Result<Vec<_>, FromRustcError>>()?,
                attr_span: self.translate_span_from_rustc(attr_span),
                was_forced: *(was_forced),
            }),
            AttributeKind::TrackCaller(_0) => Ok(from_rustc::AttributeKind::TrackCaller(
                self.translate_span_from_rustc(_0),
            )),
            _ => Err(FromRustcError),
        }
    }

    fn translate_deprecation_from_rustc(
        &mut self,
        value: &Deprecation,
    ) -> Result<from_rustc::Deprecation, FromRustcError> {
        Ok(from_rustc::Deprecation {
            since: self.translate_deprecated_since_from_rustc(&value.since)?,
            note: (&value.note)
                .as_ref()
                .map(|value| Ok(self.translate_ident_from_rustc(value)?))
                .transpose()?,
            suggestion: (&value.suggestion)
                .as_ref()
                .map(|value| Ok((value).to_string().into()))
                .transpose()?,
        })
    }

    fn translate_inline_attr_from_rustc(
        &mut self,
        value: &InlineAttr,
    ) -> Result<from_rustc::InlineAttr, FromRustcError> {
        match value {
            InlineAttr::None => Ok(from_rustc::InlineAttr::None),
            InlineAttr::Hint => Ok(from_rustc::InlineAttr::Hint),
            InlineAttr::Always => Ok(from_rustc::InlineAttr::Always),
            InlineAttr::Never => Ok(from_rustc::InlineAttr::Never),
            InlineAttr::Force { attr_span, reason } => Ok(from_rustc::InlineAttr::Force {
                attr_span: self.translate_span_from_rustc(attr_span),
                reason: (reason)
                    .as_ref()
                    .map(|value| Ok((value).to_string().into()))
                    .transpose()?,
            }),
        }
    }

    fn translate_optimize_attr_from_rustc(
        &mut self,
        value: &OptimizeAttr,
    ) -> Result<from_rustc::OptimizeAttr, FromRustcError> {
        match value {
            OptimizeAttr::Default => Ok(from_rustc::OptimizeAttr::Default),
            OptimizeAttr::DoNotOptimize => Ok(from_rustc::OptimizeAttr::DoNotOptimize),
            OptimizeAttr::Speed => Ok(from_rustc::OptimizeAttr::Speed),
            OptimizeAttr::Size => Ok(from_rustc::OptimizeAttr::Size),
        }
    }

    fn translate_deprecated_since_from_rustc(
        &mut self,
        value: &DeprecatedSince,
    ) -> Result<from_rustc::DeprecatedSince, FromRustcError> {
        match value {
            DeprecatedSince::RustcVersion(_0) => Ok(from_rustc::DeprecatedSince::RustcVersion(
                self.translate_rustc_version_from_rustc(_0)?,
            )),
            DeprecatedSince::Future => Ok(from_rustc::DeprecatedSince::Future),
            DeprecatedSince::NonStandard(_0) => Ok(from_rustc::DeprecatedSince::NonStandard(
                (_0).to_string().into(),
            )),
            DeprecatedSince::Unspecified => Ok(from_rustc::DeprecatedSince::Unspecified),
            DeprecatedSince::Err => Ok(from_rustc::DeprecatedSince::Err),
        }
    }

    fn translate_ident_from_rustc(
        &mut self,
        value: &Ident,
    ) -> Result<from_rustc::Ident, FromRustcError> {
        Ok(from_rustc::Ident {
            name: (&value.name).to_string().into(),
            span: self.translate_span_from_rustc(&value.span),
        })
    }

    fn translate_rustc_version_from_rustc(
        &mut self,
        value: &RustcVersion,
    ) -> Result<from_rustc::RustcVersion, FromRustcError> {
        Ok(from_rustc::RustcVersion {
            major: *(&value.major),
            minor: *(&value.minor),
            patch: *(&value.patch),
        })
    }
}
