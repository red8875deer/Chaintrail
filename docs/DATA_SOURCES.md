# Data sources

All sources are free. Each was checked from a development machine on **2026-11-25**.
Rate limits are mostly unpublished or change without notice, so the client uses
conservative per-host limits and backs off on HTTP 429.

## Chain data

| Chain | Primary | Fallback | Key needed | Notes |
|---|---|---|---|---|
| Bitcoin | mempool.space (Esplora API) | blockstream.info (Esplora API) | No | Address stats, address txs (25/page), tx detail with prevouts, **outspends** (needed for forward tracing), historical BTC price in USD/CAD |
| Ethereum | eth.blockscout.com (REST v2 + Etherscan-compatible API) | api.etherscan.io v2 (`chainid=1`) | Blockscout: no. Etherscan: free key | Native, internal and ERC-20 transfers; per-tx historic exchange rate; public name tags. Blockscout itself is open-source |
| Ethereum (raw) | ethereum-rpc.publicnode.com (JSON-RPC) | — | No | `eth_getCode`, `eth_call` for contract checks and token metadata |
| Tron | api.trongrid.io | apilist.tronscanapi.com | Optional free key raises limits | Account txs, TRC-20 transfers, contract events |

### Not supported (yet), and why

- **BNB Smart Chain:** no free address-history source found. Etherscan v2 returns
  *"Free API access is not supported for this chain"* for `chainid=56` (checked
  2026-11-25). Possible later via a self-hosted node.
- **Solana:** full history is too large to self-host; free APIs are too limited for tracing.

## Prices

- **BTC:** mempool.space `/api/v1/historical-price` (USD, CAD and others), full history.
- **ETH and ERC-20:** Blockscout returns a `historic_exchange_rate` per transaction.
- **Stablecoins (USDT, USDC):** treated as 1 USD, flagged if a depeg period applies.
- **CoinGecko public API:** only the last 365 days without a paid plan. Not relied on.

## Attribution (labels)

| Source | Licence / terms | Use |
|---|---|---|
| OFAC SDN list (sanctionslistservice.ofac.treas.gov) | US government work, public domain | Sanctioned addresses |
| Exchange proof-of-reserves address lists (published by the exchanges) | Published for public verification | Exchange wallets |
| Protocol documentation (official contract address lists for DEXs, bridges, mixers) | Public facts | DeFi / bridge / mixer contracts |
| GraphSense TagPacks (github.com/graphsense/graphsense-tagpacks) | Repository is MIT; **each pack must be checked individually**, some are derived from sites whose terms forbid redistribution | Candidate import, pack by pack |
| Blockscout public tags (shown live from the API) | Displayed with attribution, not copied into our dataset | Live hints |
| Our own heuristics | — | Behavioural classification (e.g. "likely exchange") |
| User-added labels | User's own | Stored locally, optionally contributed back |

**Not used:** scraping explorer label pages (e.g. Etherscan name tags). Their terms forbid
it, and it would put the project at legal risk.

## Privacy

Every public API sees the addresses you query. Self-hosting removes this:

- Bitcoin: Bitcoin Core + electrs/Esplora (~0.7 TB disk)
- Ethereum: an archive node with traces (multi-TB) or a self-hosted Blockscout
- Tron: java-tron full node (multi-TB)

All are free software; the adapters accept a custom base URL.
