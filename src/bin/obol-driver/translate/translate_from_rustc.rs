//! Conversions from rustc's own enums to the copies charon keeps in
//! `charon_lib::ast::from_rustc`.

#![allow(clippy::just_underscores_and_digits)]

extern crate rustc_ast;
extern crate rustc_attr_ir;
extern crate rustc_span;
extern crate rustc_target;

use charon_lib::ast::from_rustc::{self, FromRustcError};
use rustc_ast::attr::version::RustcVersion;
use rustc_attr_ir::lang_items::LangItem;
use rustc_hir::attrs::{
    AttributeKind, DeprecatedSince, Deprecation, InlineAttr, Linkage, OptimizeAttr,
};
use rustc_span::symbol::Ident;
use rustc_target::spec::{Arch, CfgAbi, Env, Os};

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

    pub(crate) fn translate_rustc_arch(
        &mut self,
        value: &Arch,
    ) -> Result<from_rustc::Arch, FromRustcError> {
        self.translate_arch_from_rustc(value)
    }

    pub(crate) fn translate_rustc_os(
        &mut self,
        value: &Os,
    ) -> Result<from_rustc::Os, FromRustcError> {
        self.translate_os_from_rustc(value)
    }

    pub(crate) fn translate_rustc_env(
        &mut self,
        value: &Env,
    ) -> Result<from_rustc::Env, FromRustcError> {
        self.translate_env_from_rustc(value)
    }

    pub(crate) fn translate_rustc_cfg_abi(
        &mut self,
        value: &CfgAbi,
    ) -> Result<from_rustc::CfgAbi, FromRustcError> {
        self.translate_cfg_abi_from_rustc(value)
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
            AttributeKind::ExportName { name, span } => Ok(from_rustc::AttributeKind::ExportName {
                name: (name).to_string().into(),
                span: self.translate_span_from_rustc(span),
            }),
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
            AttributeKind::LinkSection { name } => Ok(from_rustc::AttributeKind::LinkSection {
                name: (name).to_string().into(),
            }),
            AttributeKind::Linkage(_0, _1) => Ok(from_rustc::AttributeKind::Linkage(
                self.translate_linkage_from_rustc(_0)?,
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

    fn translate_arch_from_rustc(
        &mut self,
        value: &Arch,
    ) -> Result<from_rustc::Arch, FromRustcError> {
        match value {
            Arch::AArch64 => Ok(from_rustc::Arch::AArch64),
            Arch::AmdGpu => Ok(from_rustc::Arch::AmdGpu),
            Arch::Arm => Ok(from_rustc::Arch::Arm),
            Arch::Arm64EC => Ok(from_rustc::Arch::Arm64EC),
            Arch::Avr => Ok(from_rustc::Arch::Avr),
            Arch::Bpf => Ok(from_rustc::Arch::Bpf),
            Arch::CSky => Ok(from_rustc::Arch::CSky),
            Arch::Hexagon => Ok(from_rustc::Arch::Hexagon),
            Arch::LoongArch32 => Ok(from_rustc::Arch::LoongArch32),
            Arch::LoongArch64 => Ok(from_rustc::Arch::LoongArch64),
            Arch::M68k => Ok(from_rustc::Arch::M68k),
            Arch::Mips => Ok(from_rustc::Arch::Mips),
            Arch::Mips32r6 => Ok(from_rustc::Arch::Mips32r6),
            Arch::Mips64 => Ok(from_rustc::Arch::Mips64),
            Arch::Mips64r6 => Ok(from_rustc::Arch::Mips64r6),
            Arch::Msp430 => Ok(from_rustc::Arch::Msp430),
            Arch::Nvptx64 => Ok(from_rustc::Arch::Nvptx64),
            Arch::PowerPC => Ok(from_rustc::Arch::PowerPC),
            Arch::PowerPC64 => Ok(from_rustc::Arch::PowerPC64),
            Arch::RiscV32 => Ok(from_rustc::Arch::RiscV32),
            Arch::RiscV64 => Ok(from_rustc::Arch::RiscV64),
            Arch::S390x => Ok(from_rustc::Arch::S390x),
            Arch::Sparc => Ok(from_rustc::Arch::Sparc),
            Arch::Sparc64 => Ok(from_rustc::Arch::Sparc64),
            Arch::SpirV => Ok(from_rustc::Arch::SpirV),
            Arch::Wasm32 => Ok(from_rustc::Arch::Wasm32),
            Arch::Wasm64 => Ok(from_rustc::Arch::Wasm64),
            Arch::X86 => Ok(from_rustc::Arch::X86),
            Arch::X86_64 => Ok(from_rustc::Arch::X86_64),
            Arch::Xtensa => Ok(from_rustc::Arch::Xtensa),
            Arch::Other(_0) => Ok(from_rustc::Arch::Other((_0).as_ref().into())),
        }
    }

    fn translate_os_from_rustc(&mut self, value: &Os) -> Result<from_rustc::Os, FromRustcError> {
        match value {
            Os::Aix => Ok(from_rustc::Os::Aix),
            Os::AmdHsa => Ok(from_rustc::Os::AmdHsa),
            Os::Android => Ok(from_rustc::Os::Android),
            Os::Cuda => Ok(from_rustc::Os::Cuda),
            Os::Cygwin => Ok(from_rustc::Os::Cygwin),
            Os::Dragonfly => Ok(from_rustc::Os::Dragonfly),
            Os::Emscripten => Ok(from_rustc::Os::Emscripten),
            Os::EspIdf => Ok(from_rustc::Os::EspIdf),
            Os::FreeBsd => Ok(from_rustc::Os::FreeBsd),
            Os::Fuchsia => Ok(from_rustc::Os::Fuchsia),
            Os::Haiku => Ok(from_rustc::Os::Haiku),
            Os::HelenOs => Ok(from_rustc::Os::HelenOs),
            Os::Hermit => Ok(from_rustc::Os::Hermit),
            Os::Horizon => Ok(from_rustc::Os::Horizon),
            Os::Hurd => Ok(from_rustc::Os::Hurd),
            Os::Illumos => Ok(from_rustc::Os::Illumos),
            Os::IOs => Ok(from_rustc::Os::IOs),
            Os::L4Re => Ok(from_rustc::Os::L4Re),
            Os::Linux => Ok(from_rustc::Os::Linux),
            Os::LynxOs178 => Ok(from_rustc::Os::LynxOs178),
            Os::MacOs => Ok(from_rustc::Os::MacOs),
            Os::Managarm => Ok(from_rustc::Os::Managarm),
            Os::Motor => Ok(from_rustc::Os::Motor),
            Os::NetBsd => Ok(from_rustc::Os::NetBsd),
            Os::None => Ok(from_rustc::Os::None),
            Os::Nto => Ok(from_rustc::Os::Nto),
            Os::NuttX => Ok(from_rustc::Os::NuttX),
            Os::OpenBsd => Ok(from_rustc::Os::OpenBsd),
            Os::Ps3 => Ok(from_rustc::Os::Ps3),
            Os::Psp => Ok(from_rustc::Os::Psp),
            Os::Psx => Ok(from_rustc::Os::Psx),
            Os::Qnx => Ok(from_rustc::Os::Qnx),
            Os::Qurt => Ok(from_rustc::Os::Qurt),
            Os::Redox => Ok(from_rustc::Os::Redox),
            Os::Rtems => Ok(from_rustc::Os::Rtems),
            Os::Solaris => Ok(from_rustc::Os::Solaris),
            Os::SolidAsp3 => Ok(from_rustc::Os::SolidAsp3),
            Os::TeeOs => Ok(from_rustc::Os::TeeOs),
            Os::Trusty => Ok(from_rustc::Os::Trusty),
            Os::TvOs => Ok(from_rustc::Os::TvOs),
            Os::Uefi => Ok(from_rustc::Os::Uefi),
            Os::VexOs => Ok(from_rustc::Os::VexOs),
            Os::VisionOs => Ok(from_rustc::Os::VisionOs),
            Os::Vita => Ok(from_rustc::Os::Vita),
            Os::VxWorks => Ok(from_rustc::Os::VxWorks),
            Os::Wasi => Ok(from_rustc::Os::Wasi),
            Os::WatchOs => Ok(from_rustc::Os::WatchOs),
            Os::Windows => Ok(from_rustc::Os::Windows),
            Os::Xous => Ok(from_rustc::Os::Xous),
            Os::Zkvm => Ok(from_rustc::Os::Zkvm),
            Os::Unknown => Ok(from_rustc::Os::Unknown),
            Os::Other(_0) => Ok(from_rustc::Os::Other((_0).as_ref().into())),
        }
    }

    fn translate_env_from_rustc(&mut self, value: &Env) -> Result<from_rustc::Env, FromRustcError> {
        match value {
            Env::Gnu => Ok(from_rustc::Env::Gnu),
            Env::MacAbi => Ok(from_rustc::Env::MacAbi),
            Env::Mlibc => Ok(from_rustc::Env::Mlibc),
            Env::Msvc => Ok(from_rustc::Env::Msvc),
            Env::Musl => Ok(from_rustc::Env::Musl),
            Env::Newlib => Ok(from_rustc::Env::Newlib),
            Env::Nto70 => Ok(from_rustc::Env::Nto70),
            Env::Nto71 => Ok(from_rustc::Env::Nto71),
            Env::Nto71IoSock => Ok(from_rustc::Env::Nto71IoSock),
            Env::Ohos => Ok(from_rustc::Env::Ohos),
            Env::Relibc => Ok(from_rustc::Env::Relibc),
            Env::Sgx => Ok(from_rustc::Env::Sgx),
            Env::Sim => Ok(from_rustc::Env::Sim),
            Env::P1 => Ok(from_rustc::Env::P1),
            Env::P2 => Ok(from_rustc::Env::P2),
            Env::P3 => Ok(from_rustc::Env::P3),
            Env::Uclibc => Ok(from_rustc::Env::Uclibc),
            Env::V5 => Ok(from_rustc::Env::V5),
            Env::Unspecified => Ok(from_rustc::Env::Unspecified),
            Env::Other(_0) => Ok(from_rustc::Env::Other((_0).as_ref().into())),
        }
    }

    fn translate_cfg_abi_from_rustc(
        &mut self,
        value: &CfgAbi,
    ) -> Result<from_rustc::CfgAbi, FromRustcError> {
        match value {
            CfgAbi::Abi64 => Ok(from_rustc::CfgAbi::Abi64),
            CfgAbi::AbiV2 => Ok(from_rustc::CfgAbi::AbiV2),
            CfgAbi::AbiV2Hf => Ok(from_rustc::CfgAbi::AbiV2Hf),
            CfgAbi::Eabi => Ok(from_rustc::CfgAbi::Eabi),
            CfgAbi::EabiHf => Ok(from_rustc::CfgAbi::EabiHf),
            CfgAbi::ElfV1 => Ok(from_rustc::CfgAbi::ElfV1),
            CfgAbi::ElfV2 => Ok(from_rustc::CfgAbi::ElfV2),
            CfgAbi::Fortanix => Ok(from_rustc::CfgAbi::Fortanix),
            CfgAbi::Ilp32 => Ok(from_rustc::CfgAbi::Ilp32),
            CfgAbi::Ilp32e => Ok(from_rustc::CfgAbi::Ilp32e),
            CfgAbi::Llvm => Ok(from_rustc::CfgAbi::Llvm),
            CfgAbi::MacAbi => Ok(from_rustc::CfgAbi::MacAbi),
            CfgAbi::Pauthtest => Ok(from_rustc::CfgAbi::Pauthtest),
            CfgAbi::Sim => Ok(from_rustc::CfgAbi::Sim),
            CfgAbi::SoftFloat => Ok(from_rustc::CfgAbi::SoftFloat),
            CfgAbi::Spe => Ok(from_rustc::CfgAbi::Spe),
            CfgAbi::Uwp => Ok(from_rustc::CfgAbi::Uwp),
            CfgAbi::VecDefault => Ok(from_rustc::CfgAbi::VecDefault),
            CfgAbi::VecExtAbi => Ok(from_rustc::CfgAbi::VecExtAbi),
            CfgAbi::X32 => Ok(from_rustc::CfgAbi::X32),
            CfgAbi::V8Plus => Ok(from_rustc::CfgAbi::V8Plus),
            CfgAbi::Unspecified => Ok(from_rustc::CfgAbi::Unspecified),
            CfgAbi::Other(_0) => Ok(from_rustc::CfgAbi::Other((_0).as_ref().into())),
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

    fn translate_linkage_from_rustc(
        &mut self,
        value: &Linkage,
    ) -> Result<from_rustc::Linkage, FromRustcError> {
        match value {
            Linkage::AvailableExternally => Ok(from_rustc::Linkage::AvailableExternally),
            Linkage::Common => Ok(from_rustc::Linkage::Common),
            Linkage::ExternalWeak => Ok(from_rustc::Linkage::ExternalWeak),
            Linkage::External => Ok(from_rustc::Linkage::External),
            Linkage::Internal => Ok(from_rustc::Linkage::Internal),
            Linkage::LinkOnceAny => Ok(from_rustc::Linkage::LinkOnceAny),
            Linkage::LinkOnceODR => Ok(from_rustc::Linkage::LinkOnceODR),
            Linkage::WeakAny => Ok(from_rustc::Linkage::WeakAny),
            Linkage::WeakODR => Ok(from_rustc::Linkage::WeakODR),
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
