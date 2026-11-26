#!/usr/bin/env python3
"""Fail if any dependency's licence is outside the list allowed by ADR-0007.

Usage: cargo metadata --format-version 1 | python3 tools/check_licences.py [--allow LICENCE ...]

`--allow` adds licences for a specific build (e.g. MPL-2.0 for the desktop shell, ADR-0009).
"""

import json
import re
import sys

ALLOWED = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause",
    "BSD-3-Clause", "0BSD", "ISC", "Zlib", "Unicode-3.0", "CC0-1.0", "Unlicense",
    "CDLA-Permissive-2.0",
}


def allowed(expr):
    expr = expr.replace("/", " OR ")
    # Every AND term needs at least one allowed OR option.
    for term in re.split(r"\bAND\b", expr):
        options = [o.strip(" ()") for o in re.split(r"\bOR\b", term.strip(" ()"))]
        if not any(o in ALLOWED for o in options):
            return False
    return True


def main():
    args = sys.argv[1:]
    while args:
        if args[0] == "--allow" and len(args) > 1:
            ALLOWED.add(args[1])
            args = args[2:]
        else:
            sys.exit(__doc__)
    meta = json.load(sys.stdin)
    bad = [
        (p["name"], p["version"], p.get("license") or "none")
        for p in meta["packages"]
        if p["source"] is not None and not allowed(p.get("license") or "")
    ]
    for name, version, lic in bad:
        print(f"not allowed: {name} {version}: {lic}")
    print(f"{len(meta['packages'])} packages checked, {len(bad)} not allowed")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
