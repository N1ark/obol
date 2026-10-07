#!/usr/bin/env bash
# Optional end-to-end test of `obol buck2` on a standalone Buck2 project (not run by `cargo test`).
#
# It creates a small Buck2 project mirroring Meta's setup (std built from source by Buck as
# explicit sysroot deps with an empty --sysroot; `rust.extra_rustc_flags` applied to every crate),
# whose rustc is the rustup toolchain obol-driver is built with, then translates a binary, a test
# target and a library with `obol buck2` and checks the ULLBC contains std and dependency bodies.
#
# Requirements: buck2 (bundled prelude: `[external_cells] prelude = bundled`), rustup with obol's
# toolchain incl. `rust-src`, python3, gcc, a built obol (`make build`).
#
# Usage: tests/buck2/run.sh [PROJECT_DIR]     (default: a fresh temporary directory)
# Env:   BUCK2 (buck2 executable), OBOL (obol executable), BUCK2_DOTSLASH (Meta's buck2 launcher
#        needs a `.buck2` dotslash file at the project root; defaults to ~/fbsource/.buck2).
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OBOL=${OBOL:-$ROOT/bin/obol}
[ -x "$OBOL" ] || OBOL=$ROOT/target/release/obol
BUCK2=${BUCK2:-buck2}
TOOLCHAIN=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml")
PROJ=${1:-$(mktemp -d /tmp/obol-buck2-test.XXXXXX)}
mkdir -p "$PROJ"
PROJ=$(cd "$PROJ" && pwd)

echo "== setting up $PROJ (toolchain $TOOLCHAIN)"
cp -r "$HERE/project/." "$PROJ/"
rm -f "$PROJ/.buckconfig.in"
sed "s#@RUSTC@#$(rustup which rustc --toolchain "$TOOLCHAIN")#" \
    "$HERE/project/.buckconfig.in" > "$PROJ/.buckconfig"
touch "$PROJ/.buckroot"
DOTSLASH=${BUCK2_DOTSLASH:-$HOME/fbsource/.buck2}
if [ -f "$DOTSLASH" ]; then cp "$DOTSLASH" "$PROJ/.buck2"; fi
if [ ! -f "$PROJ/third-party/rust-std/BUCK" ]; then
    python3 "$HERE/gen_std_buck.py" "$TOOLCHAIN" "$PROJ"
fi

cd "$PROJ"
trap '"$BUCK2" --isolation-dir obol kill >/dev/null 2>&1 || true' EXIT
mkdir -p out
obol() { "$OBOL" buck2 --buck2 "$BUCK2" --format json "$@"; }
check() { python3 "$HERE/check_ullbc.py" "$@"; }

echo "== binary"
obol //app:app --dest-file out/app.ullbc
check out/app.ullbc --bodies-from app mylib std core alloc --entry main

echo "== rust_test target (test harness: #[test] functions are the entry points)"
obol //app:applib-unittest --dest-file out/unittest.ullbc
check out/unittest.ullbc --bodies-from mylib core --entry empty some_words

echo "== library, as a test harness (--test)"
obol --test //app:applib --dest-file out/applib-test.ullbc
check out/applib-test.ullbc --bodies-from mylib --entry empty some_words

echo "== library, public functions as entry points"
obol --start-from-pub //mylib:mylib --dest-file out/mylib.ullbc
check out/mylib.ullbc --bodies-from mylib core --entry word_count checked_double

echo "== without the MIR flags, missing dependency MIR is an error"
if obol --no-mir-flags //app:app --dest-file out/nomir.ullbc 2> out/nomir.log; then
    echo "FAILED: expected an error"; exit 1
fi
grep -q "no MIR available for .* of crate \`mylib\`" out/nomir.log
echo "ok"

echo "== all good ($PROJ)"
