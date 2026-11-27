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
- [x] Golden cases: 3 publicly documented traces replayed offline as regression tests (Colonial Pipeline, Silk Road "Individual X", Bitfinex hack)
- [x] Golden-case quotes checked against the original court documents (Colonial and Silk Road verified; Bitfinex document names no addresses)
- [x] Dormancy rule: consolidations of long-dormant coins are not mistaken for service sweeps (found by the Bitfinex golden case)
- [ ] Bridge deposit addresses (needed with Ethereum support, Phase 4)
- [ ] Fresh exchange proof-of-reserves lists directly from the exchanges (current lists date from 2022)
- [ ] Large packs as optional downloads (BitMEX 336k addresses, sextortion IOCs after licence review)

## Phase 3: Desktop app
- [x] Shared service layer (`ct-service`) and local browser mode (`chaintrail serve`, token-protected)
- [x] Tauri 2 shell with the same commands and live trace progress; Linux .deb verified
- [x] React + TypeScript UI: search, address view, transaction view, labels page
- [x] Interactive graph (Cytoscape, left-to-right by time), meaning shown by shape, colour and border
- [x] Leads panel with share of funds, confidence, basis (fact / inferred / label) and accounting
- [x] Selected lead's path highlighted; details explain every claim in plain words
- [x] Readable large graphs: small flows hidden (adaptive, with notice), branches grouped, chains collapsed, parallel transfers combined, mid-edge arrows, stable layout, readable opening zoom (ADR-0011)
- [x] All-outputs traces start from the traced transaction (exact amount to each output)
- [x] Funds followed through outputs without an address (P2PK): flows record their coins (`from_coin`, `to_coin`)
- [x] Independent audits of the trace part (correctness vs mempool.space, graph, investigator workflow, QA/accessibility) and fixes: perpetrator labels followed, CoinJoin/consolidation rules, backward per-output allocation, per-coin "not followed" leads, symmetric hops, amounts after mixing marked as pro-rata allocations
- [x] Continue a trace with the share it traced (not the whole output); request-ready endpoint details; result summary by category; history/back/reload
- [x] Exports carry provenance (time, app version, data source, label data, settings, path per lead); PNG with header; CSV safe against formulas
- [x] Keyboard use of the graph and leads; contrast; narrow screens; graph browser checks in CI (`npm run e2e`)
- [x] Large graphs (300+ addresses) laid out off the main thread; worst interaction on a 1,663-address trace under 0.5 s
- [x] Third correctness audit fixes: labelled perpetrator wallets followed however busy; Whirlpool Tx0 (premix = mixer lead, change followed); Wasabi 1.x mixing levels; postmix merges and chip splits not called CoinJoins; whole-transaction starts check their outputs as endpoints; service and backward continuations carry the right share and mixing; oversized share or minimum refused; small branches listed with their coins; unconfirmed funds at rest are inferences
- [x] Third UI audit fixes: left-to-right kept when routes are revealed (block shift); backward wording for sources; label-named nodes ringed; selecting never toggles off; notice and tools outside the graph; actions at the top of the details card; Back restores the view and reuses the trace; labelled exposure in the summary; "+X BTC hidden" on nodes; level labels on steep lines; CSV lists every transfer into an endpoint
- [x] Second UI audit fixes: records block lists every deposit; fees and unfollowed small branches shown apart (re-run with a lower minimum offered at 5%+); continuations name their source trace; router takes the least-crossing curve; "(N tx)" labels with fan stubs sized to fit; dimmed labels keep 3:1 contrast; busy notice before large redraws; start node never cut in half
- [x] Exports: report JSON, leads CSV, graph PNG
- [x] Mock mode with recorded data; UI unit tests; browser checks with Playwright
- [x] Release workflow for Windows / macOS / Linux installers (unsigned for now)
- [ ] Case files: save and reopen investigations with notes
- [ ] PDF report
- [ ] Code signing for installers
- [ ] Third-party licence notice in release bundles (ADR-0009)

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
