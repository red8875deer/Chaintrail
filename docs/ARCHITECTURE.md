# Architecture

## Constraints that shape everything

1. **Standalone desktop app.** One person on one machine. No server to run.
2. **Zero running cost.** Only free data sources. No paid APIs, no cloud database.
3. **Open source.** Every dependency must have a licence compatible with Apache-2.0.
4. **Accuracy over coverage.** A wrong link is worse than a missing one.

These rule out the approach the commercial vendors use (index every chain into a data
warehouse, then cluster the whole chain). Instead ChainTrail **fetches on demand** and
**analyses locally**, caching everything it fetches. See
[ADR-0004](decisions/0004-on-demand-analysis.md).

## Overview

```
┌──────────────────────────── Desktop app (Tauri 2) ────────────────────────────┐
│  React + TypeScript UI                                                         │
│   graph canvas · leads panel · address/tx inspector · case manager · reports   │
└───────────────────────────────▲───────────────────────────────────────────────┘
                                │ commands / events
┌───────────────────────────────┴───────────────────────────────────────────────┐
│ Rust core                                                                      │
│                                                                                │
│  ct-trace       trace engine: forward/backward, stop at endpoints, leads       │
│  ct-heuristics  clustering, change detection, CoinJoin, deposit-address, ...   │
│  ct-labels      label dataset loader + matcher (data/labels/*)                 │
│  ct-store       SQLite: cache of fetched chain data, cases, user labels        │
│  ct-providers   chain data adapters (Esplora, Blockscout, TronGrid, ...)       │
│                 shared HTTP client: rate limiting, retries, fallback           │
│  ct-core        domain model: chains, addresses, assets, amounts, txs,         │
│                 transfers, entities, labels, claims (fact/inference/label)     │
└────────────────────────────────▲──────────────────────────────────────────────┘
                                 │ HTTPS (free public APIs) or your own node
                    mempool.space · Blockscout · TronGrid · OFAC list
```

`ct-cli` exposes the same core on the command line. It is how features are built and
tested before they get a UI.

## Crates

| Crate | Status | Responsibility |
|---|---|---|
| `ct-core` | done (Bitcoin) | Domain types. No I/O. Everything else depends on it. |
| `ct-providers` | Bitcoin done | One adapter per data source, behind per-chain traits. Converts API JSON into `ct-core` types. |
| `ct-cli` | `check`, `btc address`, `btc tx`, `btc trace` | Command-line front end. |
| `ct-store` | done (Bitcoin) | SQLite cache + cases. Confirmed blocks never change, so cached txs are reused forever. |
| `ct-heuristics` | Bitcoin done | Pure functions over `ct-core` types. Each heuristic returns a scored inference with evidence. |
| `ct-labels` | planned | Loads `data/labels`, matches addresses to entities. |
| `ct-trace` | Bitcoin done | The tracing engine (see [TRACING.md](TRACING.md)). |
| `app/` | planned | Tauri 2 desktop app (Windows, macOS, Linux; mobile later from the same code). |

## Data model

Two transaction models, one graph.

- **UTXO (Bitcoin):** `UtxoTx` with inputs (spending earlier outputs) and outputs.
  Forward tracing follows *outspends* (which tx spent each output).
- **Account (Ethereum, Tron):** `Transfer` records (native, internal, token) between
  addresses. Forward tracing follows later outgoing transfers from the receiving address.

Both become **flows** in the trace graph:

```
Flow { from: Node, to: Node, asset, amount, tx, time, claim }
Node = Address | Cluster (inferred same owner) | Entity (labelled service)
```

### Claims: facts, inferences, attributions

Every piece of information carries a `Claim` describing how we know it
([ADR-0005](decisions/0005-claims-model.md)):

| Kind | Example | Confidence |
|---|---|---|
| **Fact** | "tx abc sent 0.5 BTC to bc1q…" | always certain |
| **Inference** | "bc1q… is change, so same owner as the sender" | scored, with evidence |
| **Attribution** | "1Kr6… belongs to Exchange X" | scored, with a source |

The UI draws each kind differently, and nothing is ever upgraded from inference to fact.

## Amounts

Stored as raw base units (`u128`) plus decimals: satoshis, wei, sun, token units.
`u128` holds every real balance on supported chains; values that overflow come from spam
tokens and are flagged rather than parsed.

## Networking

- One shared HTTP client: per-host rate limits, exponential backoff on 429/5xx,
  respects system proxy and CA settings.
- Every chain has a primary and fallback provider with the same API shape where possible
  (mempool.space ↔ blockstream.info are both Esplora).
- Users can point any adapter at their own node (Esplora/electrs, Ethereum RPC, Tron node)
  for privacy and speed.

## Testing strategy

- **Unit tests** on pure logic (address validation, amount maths, heuristics).
- **Fixture tests:** real API responses saved under `crates/*/tests/fixtures`, so parsing is
  tested offline and deterministically.
- **Live tests** marked `#[ignore]`; run with `cargo test -- --ignored` when online.
- **Golden cases** (Phase 2+): publicly documented traces (court filings, published
  post-mortems). The engine must reproduce their documented flows.
- CI (GitHub Actions, free for public repos): `cargo fmt --check`, `clippy -D warnings`,
  tests.
