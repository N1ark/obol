#!/usr/bin/env python3
"""Sanity checks on a JSON ULLBC file produced by `obol buck2 --format json`.

Usage: check_ullbc.py FILE [--bodies-from CRATE...] [--entry FN...]
  --bodies-from: each crate must contribute at least one function with a translated body;
  --entry: each name must be a translated local function (last path segment);
Always checks that no function body is `Missing` (i.e. every dependency had MIR).
"""
import argparse
import collections
import json
import sys


def ident_path(name):
    elems = name["name"] if isinstance(name, dict) else name
    return [e["Ident"][0] for e in elems if "Ident" in e]


def main():
    p = argparse.ArgumentParser()
    p.add_argument("file")
    p.add_argument("--bodies-from", nargs="*", default=[])
    p.add_argument("--entry", nargs="*", default=[])
    a = p.parse_args()
    t = json.load(open(a.file))["translated"]
    funs = [f for f in t["fun_decls"] if f]
    bodies = collections.Counter()
    missing = []
    local = set()
    for f in funs:
        path = ident_path(f["item_meta"]["name"])
        body = f["body"]
        kind = body if isinstance(body, str) else next(iter(body))
        if kind == "Missing":
            missing.append("::".join(path))
        elif kind == "Unstructured":
            bodies[path[0] if path else "?"] += 1
        if f["item_meta"]["is_local"] and path:
            local.add(path[-1])
    errors = []
    if missing:
        errors.append(f"{len(missing)} functions without body, e.g. {missing[:5]}")
    for c in a.bodies_from:
        if bodies[c] == 0:
            errors.append(f"no function body from crate `{c}`")
    for e in a.entry:
        if e not in local:
            errors.append(f"local function `{e}` not translated (have {sorted(local)})")
    print(f"{a.file}: {len(funs)} functions; bodies per crate: {dict(bodies)}")
    if errors:
        print("FAILED:\n  " + "\n  ".join(errors))
        sys.exit(1)


if __name__ == "__main__":
    main()
