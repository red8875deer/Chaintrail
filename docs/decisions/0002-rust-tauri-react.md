# 0002: Rust core, Tauri 2 shell, React + TypeScript UI

**Status:** Accepted (2026-11-25)

## Context
The core parses and analyses many transactions; the UI needs a fast interactive graph;
the app must run on Windows, macOS and Linux, and later on phones.

## Decision
- Core logic in Rust (workspace of small crates).
- Desktop shell: Tauri 2 (small binaries, Rust backend, supports iOS/Android later).
- UI: React + TypeScript with a WebGL graph renderer.

## Alternatives considered
- Electron: larger binaries and memory use; JavaScript core would be slower for analysis.
- Python core: fast to prototype but hard to ship as a desktop app and slower.
- Go: viable, but Tauri ties naturally to Rust and the Bitcoin/Ethereum Rust ecosystem is
  mature.

## Consequences
- One language for all analysis code; the UI calls Rust through Tauri commands.
- The CLI (`ct-cli`) reuses the same crates, so features are testable before UI work.
