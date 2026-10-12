# Obol

A simple CLI tool inspired by [Charon](https://github.com/AeneasVerif/charon/) to extract Rust information in ULLBC format. It only handles monomorphic code, but does so slightly faster and with more coverage than Charon, thanks to accessing the compiler's internals directly, rather than passing through [Hax](https://github.com/cryspen/hax/).

This tool is intended for use within [Soteria](https://github.com/soteria-tools/soteria), for Rust whole program symbolic execution, and is heavily oriented towards that, sometimes ignoring otherwise relevant information (like lifetimes or generics). If you want to do compositional analyses, use Charon instead!

## Building

Obol is a rustc driver pinned to a stable toolchain (see `rust-toolchain.toml`, installed
automatically by rustup). It relies on `rustc_private` and other unstable features, which stable
rustc only accepts with `RUSTC_BOOTSTRAP=1`; `.cargo/config.toml` sets it, so a plain
`cargo build --release` (or `make`) works. This produces the `obol` and `obol-driver` binaries in
`target/release`, which must stay next to each other. `obol` sets `RUSTC_BOOTSTRAP=1` itself when
running the toolchain.

The driver builds with rustc 1.98 and 1.99 (one branch per toolchain, differing in
`rust-toolchain.toml` and the expected UI-test outputs). `build.rs` detects the compiler and sets
`cfg(rustc_1_98)`; the few rustc API differences are confined to
`src/bin/obol-driver/rustc_compat.rs` and a handful of `#[cfg]`'d match arms and lang items.

## Usage

```sh
obol cargo [obol options] [-- <cargo options>]
obol rustc [obol options] -- <rustc options> <file.rs>
obol buck2 [obol options] <target> [-- <buck2 options>]     # see below
obol print-sysroot [--target <triple>] [--sysroot default|<path>]
```

Obol translates the standard library and the dependencies of a crate along with the crate itself,
so they must all be compiled with MIR suited for that (`obol_lib::mir_options`).

**Sysroot.** Like Charon, Obol handles the standard library implicitly: `obol cargo` and
`obol rustc` compile against a sysroot whose standard library has full MIR, built on first use
from the toolchain's `rust-src` component with Obol's MIR options (much like `cargo miri setup`,
but this is the production standard library, without `--cfg miri`). Building it takes a few
minutes and needs access to crates.io; it is then cached in
`$XDG_CACHE_HOME/obol/sysroot/<rustc version>-<commit>/<target>` (or under `$OBOL_SYSROOT_DIR`
instead of `$XDG_CACHE_HOME/obol/sysroot`). Concurrent Obol runs wait for each other (lock file)
instead of building it twice, and a cache that is incomplete or corrupt (missing or unreadable
`core`/`alloc`/`std` metadata, interrupted build) is rebuilt. Source paths are not remapped, so
the spans of standard-library items point at the toolchain's `rust-src` files (written
`/rustc/library/...` in the output, relative to `<toolchain>/lib/rustlib/src/rust`). `--sysroot` has the same semantics
as Charon's: `--sysroot default` uses the toolchain's distributed sysroot (which lacks MIR bodies
for many standard library functions), `--sysroot <path>` uses that sysroot. For `obol rustc`, a
rustc `--sysroot` argument takes precedence. Build scripts and proc-macros are always compiled
against the toolchain's own sysroot. `obol buck2` never uses this sysroot: the standard library
comes from the Buck build.

`obol print-sysroot` prints (as the last line of stdout) the sysroot `obol cargo`/`obol rustc`
would use for `--target` (default: the host), building it if needed, and exits non-zero on
failure; with `--sysroot`, it resolves the value the same way (`default` prints the toolchain's
sysroot, a path is printed back). Use it to compile crates that are linked into analysed code
(e.g. with `--extern`) against the same standard library.

**Constants.** Anonymous constants are always evaluated: inline `const { .. }` blocks become
values, and promoted constants (`&[1, 2, 3]`) become `AnonConst` globals holding the evaluated
value (no initializer function), referenced with `&`, so they have static lifetime.

## Buck2 projects: `obol buck2`

```sh
obol buck2 [OBOL OPTIONS] <TARGET> [-- <BUCK2 ARGS>]
# e.g., in fbsource:
obol buck2 --start-from-pub fbcode//foo/bar:lib -- @fbcode//mode/dev
obol buck2 fbcode//foo/bar:lib-unittest            # a rust_test: #[test] fns are the entry points
obol buck2 --test fbcode//foo/bar:lib              # the same: lib-unittest, if it exists
```

`TARGET` is a `rust_library`, `rust_binary` or `rust_test`. With `--test`, a library is compiled as
a test harness; if it has a `<TARGET>-unittest` test target (fbcode's `rust_library` defines one,
with the library's `test_deps`), that target is translated instead. The usual translation options apply
(`--dest-file`, `--format`, `--start-from`, `--start-from-attribute`, `--start-from-pub`,
`--opaque`, `--print-ullbc`, ...); the output defaults to `<crate>.ullbc.postcard` in the current
directory. Arguments after `--` go to `buck2 build`/`buck2 aquery` (mode files, `-c` options).
Buck2-specific options: `--test`, `--buck2 <exe>`, `--isolation-dir <dir>` (default `obol`),
`--print-command`, `--skip-version-check`, `--allow-missing-mir`, `--no-mir-flags`.

Nothing has to be added to the Buck project; only the `buck2` CLI is used:

1. **Dependencies and std with full MIR.** Obol reads dependency bodies from their metadata, which
   only contains MIR for every function when compiled with Obol's MIR options. `obol buck2` builds
   `<TARGET>[check]` with
   `-c rust.extra_rustc_flags='-Zalways-encode-mir -Zmir-opt-level=0 -Zmir-preserve-ub -Zmir-enable-passes=-CheckAlignment,-CheckNull,-CheckEnums,-RemoveStorageMarkers'`,
   which Buck applies to every crate (metadata only, no codegen). When the toolchain builds std
   from source as explicit sysroot deps (Meta's fbcode toolchain does), std/core/alloc get full
   MIR too: this is the production std (no `cfg(miri)`). The build runs in a dedicated
   `--isolation-dir` so that the config change doesn't invalidate your usual daemon; stop it with
   `buck2 --isolation-dir obol kill`. The first build of a target rebuilds its whole graph,
   std included (minutes); later runs are incremental.
2. **The crate's exact rustc command line.** `buck2 aquery` gives the target's metadata compile
   action and its argsfile; Obol finds the materialized argsfile in `buck-out` (built with
   `-c buck2.defer_write_actions=false`), asks Buck to materialize every input it references
   (`buck2 debug materialize`, needed with remote execution), and parses Buck's
   `rustc_action.py` conventions (`--env`, `--path-env`, `--env-set`, `$(cat ...)`, nested
   `@argsfiles`, paths relative to the project root).
3. **Translation.** `obol-driver` runs from the project root on those arguments, with the action's
   environment, in explicit translate mode, and writes the ULLBC.

**Version requirement.** `obol-driver` is a rustc and can only load metadata (and proc macros)
produced by the *identical* compiler. `obol buck2` reads the compiler version from the
dependencies' `.rmeta` headers and stops with a clear error if it differs from obol-driver's
(`obol-driver rustc --version`). For fbsource, Obol must therefore be built with the toolchain
matching Meta's rustc (rustup `1.98.1` / `1.99.x` + `rustc-dev`, same `rustc -vV` as Buck's
buckified compiler).

**Limitations.**
* The full-MIR graph relies on the toolchain honoring `rust.extra_rustc_flags` (fbcode's does; a
  custom toolchain must pass it as `extra_rustc_flags`). `obol buck2` sets that config, replacing
  any value you set yourself. With `--no-mir-flags`, make sure the graph has full MIR some other
  way; functions of dependencies without MIR make the translation fail (unless
  `--allow-missing-mir`).
* With a toolchain using a prebuilt sysroot (e.g. the prelude's `system_rust_toolchain`), std
  is not rebuilt and only its generic/inline functions have MIR.
* The translation runs locally, not as a (cacheable, remote) Buck action; see the CI notes in the
  Soteria planning docs.
* `rustc_action.py`'s behavior is reimplemented, not reused: changes to the prelude's wrapper
  conventions may need updating `obol_lib::rustc_action`.

**Driver interface** (for build-system integrations, e.g. a future BXL/rule running obol-driver
as the compiler of a Buck action): `obol-driver` accepts being invoked as
`obol-driver [rustc] <args|@argsfile>` and reads, besides `OBOL_ARGS`:
`OBOL_MODE=translate` (always translate this crate, no codegen) or `OBOL_MODE=rustc` (compile
normally with Obol's MIR options), `OBOL_DEST` (output file), `OBOL_ARGS_FILE` (JSON options in a
file) and `OBOL_ALLOW_MISSING_MIR`. Standard-library crates are recognized by crate name, so the
output uses `/rustc/library/...` paths for std whatever its location.

**Testing.** `tests/buck2/run.sh [DIR]` builds a standalone Buck2 project (std buckified from
`rust-src` by `tests/buck2/gen_std_buck.py`, toolchain = Obol's rustup toolchain) and checks
translations of a binary, a test target and a library.

## License

Obol is licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE).
