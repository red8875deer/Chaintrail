# 0004: On-demand fetching and local analysis

**Status:** Accepted (2026-11-25)

## Context
Commercial tools index every transaction on every chain, then cluster the whole chain.
That needs terabytes of storage and servers, which 0001 rules out.

## Decision
Fetch only what a trace needs, from free APIs, and cache it locally in SQLite.
Run heuristics (clustering, change detection, service detection) on the fetched
neighbourhood only.

## Consequences
- Works on a laptop with no setup.
- Confirmed blocks never change, so the cache only grows more useful over time.
- Clusters are partial (built from what was touched), so the tool under-merges rather than
  over-merges. That is the safe failure for accuracy.
- Very large services (exchange hot wallets with millions of txs) cannot be fully fetched;
  they are recognised as endpoints by label or behaviour and tracing stops there, which is
  what an investigator wants anyway.
- A future "full node mode" can plug in through the same provider traits.
