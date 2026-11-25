# 0001: Standalone desktop app with zero running cost

**Status:** Accepted (2026-11-25)

## Context
The owner wants to use the tool personally, test it for free, and later share it freely.
Commercial tracing tools run large server clusters over full-chain indexes.

## Decision
Ship a desktop application that runs entirely on the user's machine, uses only free data
sources, and stores everything locally. No backend service.

## Consequences
- Anyone can run it without an account or payment.
- Data fetching is limited by free API rate limits, so deep traces are slower than in
  commercial tools; caching mitigates repeat work.
- Whole-chain analytics (global clustering) are not possible without a self-hosted node;
  see 0004.
- A shared/hosted mode can be added later without changing the core crates.
