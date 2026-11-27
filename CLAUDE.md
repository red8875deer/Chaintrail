# ChainTrail – notes for Claude sessions

Free, open-source crypto tracing desktop app (alternative to commercial tools).
Owner has limited time: Claude does most of the implementation. Keep docs current so the
next session can continue without re-deriving context.

## Read first
- `docs/ROADMAP.md` – what is done, what is next (tick boxes as you finish items)
- `docs/ARCHITECTURE.md` – crates and data model
- `docs/TRACING.md` – heuristics and confidence rules (code must follow it)
- `docs/DATA_SOURCES.md` – free APIs, limits, what is not allowed (no scraping)
- `docs/decisions/` – ADRs; add a new one for any significant decision

## Hard constraints
- Zero running cost: free data sources only; standalone desktop app; no servers.
- Accuracy first: never present an inference or label as a fact. Every inference carries
  evidence + confidence (see `ct_core::claim`).
- Licences: dependencies and imported label data must be compatible with Apache-2.0.
- Never put personal data, API keys or the owner's identity in code, commits or requests.

## Layout
- `crates/ct-core` – domain types, no I/O
- `crates/ct-providers` – API adapters → `ct-core` types; fixtures in `tests/fixtures`
- `crates/ct-store` – SQLite cache wrapping any provider (`CachedBitcoin`)
- `crates/ct-heuristics` – pure heuristics (CoinJoin, change, shape, services, clusters)
- `crates/ct-trace` – trace engine + `testing` module (in-memory provider, tx builders)
- `crates/ct-cli` – `chaintrail` binary
- `crates/ct-labels` – label packs (TOML), index, OFAC importer; `build.rs` embeds `data/labels/*.toml`
- `data/labels` – built-in label packs + `SOURCES.md` (generated)
- `tools/import_graphsense.py` – regenerates `gs-*.toml` packs from a GraphSense checkout
- `crates/ct-service` – `AppService`: every UI command; `views.rs` is the UI contract
- `app/ui` – React + TS + Cytoscape UI; `src/api/types.ts` mirrors `views.rs` (change both together);
  `fixtures/` are real responses for mock mode and tests
- `app/src-tauri` – Tauri 2 shell (own Cargo workspace; MPL deps allowed here only, ADR-0009)

## Commands
```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                 # offline
cargo test --workspace -- --ignored    # live API tests (needs network)
cargo run -p ct-cli -- btc address <addr>
cargo run -p ct-cli -- btc trace <txid> [--vout N | --to ADDR] [--backward]
cargo run -p ct-cli -- labels lookup <addr> | labels packs | labels add ... | labels update-ofac
# regenerate built-in OFAC snapshot: labels update-ofac --file SDN.XML --out data/labels/ofac-sdn.toml
# regenerate GraphSense packs: python3 tools/import_graphsense.py <graphsense-tagpacks checkout> data/labels
cargo run -p ct-cli -- serve --port 7979 --token devtoken   # API + built UI for browser testing
(cd app/ui && npm ci && npm run licences && npm test && npm run build)
(cd app/ui && npm run e2e)            # graph browser checks on recorded traces (after build)
(cd app/ui && npm run dev)            # http://localhost:5173/#token=devtoken (proxies /api to 7979)
http://localhost:5173/?mock=1         # UI on recorded fixtures, no network
cargo clippy --manifest-path app/src-tauri/Cargo.toml -- -D warnings
(cd app/src-tauri && npx @tauri-apps/cli@2 build --bundles deb)   # Linux installer
```
Linux GUI testing: `xvfb-run` + `import -window root` screenshots; Playwright is pre-installed.

## Conventions
- Rust 2024 edition. Errors via `thiserror` in libraries, `anyhow` in the CLI.
- Amounts: raw base units `u128` + decimals. Never floats for amounts.
- New API parsing → save a real response as a fixture and test against it.
- Small commits; update ROADMAP checkboxes in the same commit as the feature.
- Trace engine invariant: leads + fees + below-threshold = seed amount (tests assert it).
- Heuristic thresholds/weights live as named constants next to their tests; change them
  together with `docs/TRACING.md`.
- Dependency licences: see ADR-0007 (no copyleft without a decision).
- UI: amounts are integer sats, formatted with BigInt helpers in `app/ui/src/lib/format.ts`;
  never present an inference or label as a fact (use `describeClaim`, `Basis`).
- Label data: only sources allowed by ADR-0008; never scrape explorer label pages. Add the
  pack to the allowlist in `tools/import_graphsense.py` or document the source in SOURCES.md.
