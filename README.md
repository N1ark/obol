# Obol

A simple CLI tool inspired by [Charon](https://github.com/AeneasVerif/charon/) to extract Rust information in ULLBC format. It only handles monomorphic code, but does so slightly faster and with more coverage than Charon, thanks to accessing the compiler's internals directly, rather than passing through [Hax](https://github.com/cryspen/hax/).

This tool is intended for use within [Soteria](https://github.com/soteria-tools/soteria), for Rust whole program symbolic execution, and is heavily oriented towards that, sometimes ignoring otherwise relevant information (like lifetimes or generics). If you want to do compositional analyses, use Charon instead!

## Sysroot

Obol translates the standard library along with the crate, so it needs MIR for the standard
library's functions. Like Charon, `obol cargo` and `obol rustc` therefore compile against a sysroot
whose standard library has full MIR by default: it is built on first use from the toolchain's
`rust-src` component with Obol's MIR options (`obol_lib::mir_options`), much like
`cargo miri setup`, but without `--cfg miri` (this is the production standard library). Building
it takes a few minutes and needs access to crates.io; it is then cached in
`$XDG_CACHE_HOME/obol/sysroot/<rustc version>-<commit>-<MIR options hash>/<target>` (or under
`$OBOL_SYSROOT_DIR` instead of `$XDG_CACHE_HOME/obol/sysroot`). Concurrent Obol runs wait for each
other instead of building it twice, and an incomplete or corrupt cache is rebuilt.

`obol print-sysroot [--target <triple>] [--sysroot default|<path>]` prints the sysroot
`obol cargo`/`obol rustc` would use, building it if needed. Use it to compile crates that are
linked into analysed code (e.g. with `--extern`) against the same standard library.

## License

Obol is licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE).
