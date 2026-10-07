# A Rust toolchain for the standalone `obol buck2` test project, modelled after Meta's fbcode
# toolchain (`fbcode/buck2/platform/rust/rust.bzl`):
#   * the compiler is an absolute path (the rustup rustc matching obol-driver);
#   * std & co. are ordinary `rust_library` targets passed as explicit sysroot deps, with an empty
#     `--sysroot`;
#   * `rust.extra_rustc_flags` (buckconfig) is appended to every compile, sysroot crates included.

load("@prelude//rust:rust_toolchain.bzl", "PanicRuntime", "RustExplicitSysrootDeps", "RustToolchainInfo")

def _toolchain_impl(ctx):
    sysroot = None
    if ctx.attrs.core:
        sysroot = RustExplicitSysrootDeps(
            core = ctx.attrs.core,
            std = ctx.attrs.std,
            proc_macro = ctx.attrs.proc_macro,
            panic_unwind = ctx.attrs.panic_unwind,
            panic_abort = ctx.attrs.panic_abort,
            others = ctx.attrs.others,
        )
    return [
        DefaultInfo(),
        RustToolchainInfo(
            compiler = RunInfo(args = [ctx.attrs.compiler]),
            clippy_driver = RunInfo(args = ["clippy-driver"]),
            rustdoc = RunInfo(args = ["rustdoc"]),
            default_edition = "2021",
            panic_runtime = PanicRuntime("unwind"),
            nightly_features = True,
            rustc_target_triple = ctx.attrs.rustc_target_triple,
            rustc_flags = ctx.attrs.rustc_flags,
            extra_rustc_flags = ctx.attrs.extra_rustc_flags,
            explicit_sysroot_deps = sysroot,
        ),
    ]

obol_test_rust_toolchain = rule(
    impl = _toolchain_impl,
    attrs = {
        "compiler": attrs.string(),
        "core": attrs.option(attrs.dep(), default = None),
        "extra_rustc_flags": attrs.list(attrs.string(), default = []),
        "others": attrs.list(attrs.dep(), default = []),
        "panic_abort": attrs.option(attrs.dep(), default = None),
        "panic_unwind": attrs.option(attrs.dep(), default = None),
        "proc_macro": attrs.option(attrs.dep(), default = None),
        "rustc_flags": attrs.list(attrs.string(), default = []),
        "rustc_target_triple": attrs.string(default = "x86_64-unknown-linux-gnu"),
        "std": attrs.option(attrs.dep(), default = None),
    },
    is_toolchain_rule = True,
)
