# 0003: First chains: Bitcoin, Ethereum, Tron

**Status:** Accepted (2026-11-25)

## Context
The owner asked for the top five assets (Bitcoin, Ether, Tether and two more), under a
zero-cost constraint.

## Decision
Support three chains and five assets first:

| Asset | Chains |
|---|---|
| BTC | Bitcoin |
| ETH | Ethereum |
| USDT | Ethereum, Tron |
| USDC | Ethereum |
| TRX | Tron |

Token support is generic (any ERC-20 / TRC-20), but testing focuses on these five.

## Reasons
- These carry most of the value in scam, fraud and ransomware cases. USDT on Tron is the
  dominant channel for scam proceeds, so Tron is included from the start.
- Each has a free address-history API (see DATA_SOURCES.md).
- Three adapters cover all five assets.

## Rejected for now
- **BNB / BNB Smart Chain:** Etherscan v2 free tier explicitly excludes it
  (verified 2026-11-25); no other free address-history source found.
- **XRP, Solana:** separate data models and less free tooling; revisit later.
