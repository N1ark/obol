#!/usr/bin/env python3
"""Buckify the Rust standard library of a rustup toolchain, for the standalone Buck2 test project.

Meta's Buck2 setup builds std from source like any other crate (explicit sysroot deps + an empty
`--sysroot`); this script reproduces that in a small project so that `obol buck2` can be tested
outside fbsource. It runs `cargo -Zbuild-std` once on a dummy crate (offline: rust-src ships the
vendored std dependencies), records the rustc invocations (crate names, features, cfgs emitted by
build scripts, extern edges, source files from the dep-info), copies `library/` into the project
and writes one `rust_library` per std crate.

Usage: gen_std_buck.py <toolchain> <project-dir> [<package> = third-party/rust-std]
"""

import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile


def main():
    toolchain, project = sys.argv[1], os.path.abspath(sys.argv[2])
    package = sys.argv[3] if len(sys.argv) > 3 else "third-party/rust-std"
    sysroot = subprocess.check_output(
        ["rustc", f"+{toolchain}", "--print", "sysroot"], text=True
    ).strip()
    lib = os.path.join(sysroot, "lib/rustlib/src/rust/library")
    host = re.search(
        r"host: (\S+)",
        subprocess.check_output(["rustc", f"+{toolchain}", "-vV"], text=True),
    ).group(1)

    def map_src(path):
        """Map a source path seen by cargo to a path relative to the copied `library/`."""
        path = os.path.normpath(path)
        if path.startswith(lib + "/"):
            return os.path.join("library", os.path.relpath(path, lib))
        # Vendored std dependencies may be unpacked into the cargo registry; rust-src ships
        # identical copies under `library/vendor`.
        m = re.search(r"/registry/src/[^/]+/([^/]+)/(.*)$", path)
        if m and os.path.exists(os.path.join(lib, "vendor", m.group(1), m.group(2))):
            return os.path.join("library/vendor", m.group(1), m.group(2))
        return None

    work = tempfile.mkdtemp(prefix="obol-buildstd-")
    subprocess.check_call(
        ["cargo", f"+{toolchain}", "init", "-q", "--name", "dummy", work]
    )
    log = subprocess.run(
        [
            "cargo", f"+{toolchain}", "check", "-v", "--offline",
            "-Zbuild-std=std,panic_abort,panic_unwind,proc_macro,test",
            "--target", host,
            # Take std's dependencies from rust-src's `library/vendor` even if they aren't in the
            # local registry cache (older cargos don't do that by themselves).
            "--config", "source.crates-io.replace-with='rust-src-vendor'",
            "--config", f"source.rust-src-vendor.directory='{lib}/vendor'",
        ],
        cwd=work, capture_output=True, text=True,
        # `-Zbuild-std` is unstable; Obol's toolchain may be a stable release.
        env=dict(os.environ, RUSTC_BOOTSTRAP="1"),
    )
    if log.returncode != 0:
        sys.exit(f"cargo -Zbuild-std failed:\n{log.stderr}")

    crates = {}  # out-file stem (lib<name>-<hash>) -> info
    for line in log.stderr.splitlines():
        line = line.strip()
        if not line.startswith("Running `"):
            continue
        argv = shlex.split(line[len("Running `"):-1])
        if "--crate-name" not in argv:
            continue
        name = argv[argv.index("--crate-name") + 1]
        if name in ("build_script_build", "dummy"):
            continue
        info = {
            "name": name, "edition": "2015", "features": [], "flags": [],
            "externs": [], "root": None, "deps_file": None,
        }
        i = 1
        while i < len(argv):
            a = argv[i]
            nxt = argv[i + 1] if i + 1 < len(argv) else None
            if a.startswith("--edition="):
                info["edition"] = a.split("=", 1)[1]
            elif a == "--cfg":
                m = re.fullmatch(r'feature="(.*)"', nxt)
                if m:
                    info["features"].append(m.group(1))
                else:
                    info["flags"].append(f"--cfg={nxt}")
                i += 1
            elif a == "--extern":
                opts, _, rest = nxt.rpartition(":") if ":" in nxt.split("=")[0] else ("", "", nxt)
                ename, path = rest.split("=", 1)
                stem = os.path.basename(path).rsplit(".", 1)[0]
                info["externs"].append((ename, stem))
                i += 1
            elif a == "-Z" and nxt == "force-unstable-if-unmarked":
                info["flags"].append("-Zforce-unstable-if-unmarked")
                i += 1
            elif a == "-C" and nxt.startswith("extra-filename="):
                info["stem"] = f"lib{name}{nxt.split('=', 1)[1]}"
                info["dep_info"] = f"{name}{nxt.split('=', 1)[1]}.d"
                i += 1
            elif a == "--out-dir":
                info["out_dir"] = nxt
                i += 1
            elif a in ("--check-cfg", "-C", "-Z", "--crate-type", "--error-format",
                       "--target", "-L", "--cap-lints", "--emit"):
                i += 1
            elif a.endswith(".rs") and not a.startswith("-"):
                info["root"] = map_src(a)
            i += 1
        crates[info["stem"]] = info

    # Copy the library sources into the project.
    dest = os.path.join(project, package)
    os.makedirs(dest, exist_ok=True)
    if os.path.exists(os.path.join(dest, "library")):
        shutil.rmtree(os.path.join(dest, "library"))
    shutil.copytree(lib, os.path.join(dest, "library"), symlinks=False)

    by_stem_name = {stem: info["name"] for stem, info in crates.items()}
    target_names = {}
    for stem, info in crates.items():
        tname = info["name"]
        assert tname not in target_names.values(), f"duplicate crate {tname}"
        target_names[stem] = tname

    out = [
        "# @generated by obol/tests/buck2/gen_std_buck.py; do not edit.",
        f"# Standard library of {toolchain}, built from source like Meta's buckified sysroot.",
        "",
    ]
    for stem, info in sorted(crates.items(), key=lambda kv: kv[1]["name"]):
        srcs = set()
        with open(os.path.join(info["out_dir"], info["dep_info"])) as f:
            for dline in f:
                if dline.strip() == "" or ":" not in dline:
                    continue
                for p in dline.split(":", 1)[1].split():
                    m = map_src(p)
                    if m is not None:
                        srcs.add(m)
        deps, named = [], {}
        for ename, dstem in info["externs"]:
            target = ":" + target_names[dstem]
            if ename == by_stem_name[dstem]:
                deps.append(target)
            else:
                named[ename] = target
        env = {}
        if info["name"] == "std":
            env["STD_ENV_ARCH"] = host.split("-")[0]
        flags = info["flags"] + ["--cap-lints=allow"]
        out.append("rust_library(")
        out.append(f'    name = "{target_names[stem]}",')
        out.append(f'    crate = "{info["name"]}",')
        out.append(f'    crate_root = "{info["root"]}",')
        out.append(f'    edition = "{info["edition"]}",')
        # Include whole packages (nearest dir with a Cargo.toml) rather than the exact dep-info
        # files: some crates use `#[path = "a/../b"]`, which needs the intermediate dirs.
        pkgs = set()
        for src in srcs:
            d = os.path.dirname(src)
            while d and not os.path.exists(os.path.join(lib, os.path.relpath(d, "library"), "Cargo.toml")):
                d = os.path.dirname(d)
            pkgs.add(d or os.path.dirname(src))
        globs = [f"{d}/**" for d in sorted(pkgs)]
        out.append(f"    srcs = glob({json.dumps(globs)}),")
        out.append(f"    features = {json.dumps(sorted(set(info['features'])))},")
        out.append(f"    rustc_flags = {json.dumps(flags)},")
        out.append(f"    deps = {json.dumps(sorted(set(deps)))},")
        if named:
            out.append(f"    named_deps = {json.dumps(named, sort_keys=True)},")
        if env:
            out.append(f"    env = {json.dumps(env)},")
        out.append('    preferred_linkage = "static",')
        out.append('    _rust_toolchain = "toolchains//:rust_bootstrap",')
        out.append('    visibility = ["PUBLIC"],')
        out.append(")")
        out.append("")
    with open(os.path.join(dest, "BUCK"), "w") as f:
        f.write("\n".join(out))
    shutil.rmtree(work)
    print(f"wrote {len(crates)} std crates to {dest}/BUCK")


if __name__ == "__main__":
    main()
