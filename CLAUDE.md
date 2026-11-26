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
- `data/labels` – open label dataset (planned)

## Commands
```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                 # offline
cargo test --workspace -- --ignored    # live API tests (needs network)
cargo run -p ct-cli -- btc address <addr>
cargo run -p ct-cli -- btc trace <txid> [--vout N | --to ADDR] [--backward]
```

## Conventions
- Rust 2024 edition. Errors via `thiserror` in libraries, `anyhow` in the CLI.
- Amounts: raw base units `u128` + decimals. Never floats for amounts.
- New API parsing → save a real response as a fixture and test against it.
- Small commits; update ROADMAP checkboxes in the same commit as the feature.
- Trace engine invariant: leads + fees + below-threshold = seed amount (tests assert it).
- Heuristic thresholds/weights live as named constants next to their tests; change them
  together with `docs/TRACING.md`.
- Dependency licences: see ADR-0007 (no copyleft without a decision).
