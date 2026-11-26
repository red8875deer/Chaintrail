# 0007: Dependency licences

**Status:** Accepted (2026-11-26)

## Context
ChainTrail is Apache-2.0 (ADR-0006) and should be usable by anyone, including public
agencies, without licence surprises.

## Decision
Dependencies (including transitive ones) may use: MIT, MIT-0, Apache-2.0, BSD-2/3-Clause,
0BSD, ISC, Zlib, Unicode-3.0, CC0, Unlicense, public domain, and CDLA-Permissive-2.0 (a
permissive data licence, used by the TLS root-certificate bundle). A dual licence counts
if one of its options is allowed. Anything else, including MPL-2.0, LGPL, GPL or AGPL,
needs a new decision record before it is added.

Checked on 2026-11-26: every dependency in `Cargo.lock` meets this.

## Consequences
- `ct-store` computes the user data folder itself instead of using the `dirs` crate,
  because `dirs` pulls in `option-ext` (MPL-2.0).
- The Tauri desktop shell may bring MPL-2.0 dependencies. MPL-2.0 is file-level copyleft
  and is commonly accepted for unmodified dependencies of Apache-2.0 projects; decide when
  Phase 3 starts.
- CI runs `tools/check_licences.py` on every push.
