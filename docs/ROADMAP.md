# Roadmap

Each phase ends with something usable and must pass its tests before the next starts.
Dates are not committed; quality is.

## Phase 0: Foundations
- [x] Architecture, tracing methodology, data sources, decision records
- [x] Rust workspace, CI (fmt, clippy, tests)
- [x] `ct-core` domain model: chains, addresses (with checksum validation), assets, amounts, claims, UTXO txs
- [x] Bitcoin provider (Esplora: mempool.space / blockstream.info) with fixture tests
- [x] CLI: `btc address`, `btc tx`
- [x] Move to the dedicated GitHub repository

## Phase 1: Bitcoin tracing (CLI)
- [ ] `ct-store`: SQLite cache for txs, outspends, address pages
- [ ] Provider fallback (mempool.space → blockstream.info) and shared rate limiter
- [ ] Heuristics: script-type, CoinJoin detection, change detection, peel chains
- [ ] Forward and backward trace with hop/value limits
- [ ] Behavioural service detection (consolidations, high fan-in) → "likely exchange"
- [ ] Leads output (CLI table + JSON)
- [ ] First golden cases from public documents

## Phase 2: Labels
- [ ] Label file format with source, date, confidence, licence per entry
- [ ] OFAC SDN importer
- [ ] Exchange proof-of-reserves addresses (major exchanges)
- [ ] Known mixer / bridge / DEX contracts
- [ ] GraphSense TagPacks import, licence-checked pack by pack
- [ ] User labels stored locally

## Phase 3: Desktop app
- [ ] Tauri 2 shell, React + TypeScript UI
- [ ] Graph canvas (WebGL), left-to-right time-ordered layout
- [ ] Leads panel, address/tx inspector, case save/load
- [ ] One-click "trace forward / trace backward" from any node

## Phase 4: Ethereum and Tron
- [ ] Blockscout adapter (native, internal, ERC-20 transfers)
- [ ] TronGrid adapter (TRX, TRC-20)
- [ ] Deposit-address detection, gas-funding links
- [ ] Allocation rules: pro-rata (default), FIFO, LIFO, poison, LIBR
- [ ] DEX swap decoding (value continues in output token)

## Phase 5: Cross-chain and reports
- [ ] Bridge matching (deterministic where the protocol allows, probable otherwise)
- [ ] Report export: PDF + CSV/XLSX with path, amounts, evidence, confidence
- [ ] Case reproducibility (pinned block heights and label version)

## Later
- Watchlists and alerts when traced funds move
- Self-hosted node presets for privacy
- More chains (e.g. BNB Smart Chain via own node, Litecoin via Esplora)
- CoinJoin demixing, ML-based entity classification
- Mobile app (same Tauri codebase)
- Court-oriented reporting (methodology appendix, error rates)
