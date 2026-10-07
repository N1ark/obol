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

## Usage

```sh
obol setup [--target <triple>] [--print-sysroot]
obol cargo [obol options] [-- <cargo options>]
obol rustc [obol options] -- <rustc options> <file.rs>
```

Obol translates the standard library and the dependencies of a crate along with the crate itself,
so they must all be compiled with MIR suited for that (`obol_lib::mir_options`). For the standard
library, `obol setup` builds a sysroot from the toolchain's `rust-src` component with these options
(much like `cargo miri setup`, but this is the production standard library, without `--cfg miri`).
Building it requires access to crates.io; it is cached in
`$XDG_CACHE_HOME/obol/sysroot/<rustc version>-<commit>/<target>` (or under `$OBOL_SYSROOT_DIR`
instead of `$XDG_CACHE_HOME/obol/sysroot`), so later calls are fast. With `--print-sysroot`, the
last line of stdout is the path to the sysroot.

`obol cargo` and `obol rustc` use that sysroot (building it if needed) unless told otherwise with
`--sysroot <path>` or `--sysroot default` (the toolchain's own sysroot); for `obol rustc`, a rustc
`--sysroot` argument takes precedence. Build scripts and proc-macros are always compiled against
the toolchain's own sysroot.

## License

Obol is licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE).
