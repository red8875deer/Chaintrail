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
- [x] `ct-store`: SQLite cache for txs, outspends, address pages (confirmed data kept forever)
- [x] Provider fallback (mempool.space → blockstream.info) and shared rate limiter
- [x] Heuristics: tx shape, CoinJoin detection (Whirlpool, WabiSabi, generic), change detection, peel chains, clustering with evidence
- [x] Forward and backward trace with hop / transaction / value limits, pro-rata allocation
- [x] Behavioural service detection (busy wallets, deposit consolidations, batch payouts) → "likely service"
- [x] Leads output (CLI + JSON) with accounting that always adds up to the traced amount
- [ ] Golden cases from public documents (moved to Phase 2: needs sourced labels)

### Follow-ups found during Phase 1
- [ ] Reorg safety: cache transactions forever only after ~6 confirmations
- [ ] Calibrate heuristic weights against labelled data and publish error rates
- [ ] Change detection: fresh-address and fiat-round-amount signals
- [ ] Let the user "continue through" a detected service or CoinJoin from a lead

## Phase 2: Labels
- [x] Label pack format (TOML) with source, publisher, licence, date and confidence; packs validated in tests
- [x] OFAC SDN importer (`labels update-ofac`), built-in snapshot of 1,027 addresses
- [x] Exchange-published wallet lists: Bitfinex, OKX, Huobi, Binance, KuCoin, Bybit, Deribit, Crypto.com, Checksig (via GraphSense)
- [x] Mixers (ChipMixer, Blender, Sinbad, Wasabi fees, Tornado Cash), darknet markets, ransomware, hacks, scams, mining pools, DeFi contracts
- [x] GraphSense TagPacks import, reviewed pack by pack (`tools/import_graphsense.py`, `data/labels/SOURCES.md`)
- [x] User labels (`labels add`) stored locally; same-id packs replace built-in ones
- [x] Traces name detected services through labels on addresses of the same wallet
- [ ] Golden cases: publicly documented traces reproduced as regression tests
- [ ] Bridge deposit addresses (needed with Ethereum support, Phase 4)
- [ ] Fresh exchange proof-of-reserves lists directly from the exchanges (current lists date from 2022)
- [ ] Large packs as optional downloads (BitMEX 336k addresses, sextortion IOCs after licence review)

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
