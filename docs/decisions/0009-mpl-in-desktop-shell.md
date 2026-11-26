# 0009: MPL-2.0 dependencies allowed in the desktop shell only

**Status:** Accepted (2026-11-26). The owner may reverse it; the impact is limited to `app/src-tauri`.

## Context
ADR-0007 allows only permissive licences and requires a decision for anything else. The
Tauri desktop shell (ADR-0002) depends on seven MPL-2.0 crates: `cssparser`, `cssparser-macros`,
`selectors` and `dtoa-short` (Servo's CSS parser, used by Tauri's HTML processing) and
`option-ext` (via `dirs`). Tauri cannot be built without them.

## Decision
MPL-2.0 is allowed for **unmodified third-party dependencies of the desktop shell**
(`app/src-tauri`). The core crates (`crates/*`) stay permissive-only, checked in CI.

MPL-2.0 is file-level copyleft: it only requires that modified MPL files are published and
that recipients of binaries can obtain the MPL source. We do not modify these files, and
their source is public on crates.io. It is explicitly designed to be combined with code
under other licences.

## Consequences
- CI checks the desktop shell with `tools/check_licences.py --allow MPL-2.0`.
- Release bundles must include a third-party licence notice listing these crates and where
  to get their source (to do before the first public release; e.g. generated with
  `cargo about`).
- Anything copyleft beyond MPL-2.0 (LGPL, GPL, AGPL, EPL) still needs its own decision.
