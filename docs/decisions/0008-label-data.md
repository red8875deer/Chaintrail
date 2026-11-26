# 0008: Label data sources and format

**Status:** Accepted (2026-11-26)

## Context
Knowing who owns an address is what turns a trace into leads. Commercial vendors build
this from years of private investigations. ChainTrail must do it with open, lawful data,
and must never present a label as more certain than its source.

## Decision
- **Format:** TOML label packs with source, publisher, licence, date, default category and
  confidence, and groups of addresses per entity. Built-in packs in `data/labels/` are
  embedded at compile time; user packs live in the user data folder. A pack with the same
  id as a built-in one replaces it (used for fresh OFAC downloads).
- **Allowed sources:** government and court publications; data published by the entity
  itself (e.g. exchange proof-of-reserves lists); published research datasets; the user's
  own labels. Collections such as GraphSense TagPacks are used only pack by pack after
  checking the original source of each pack.
- **Not allowed:** scraping explorer label pages or other sites whose terms forbid reuse;
  data containing personal information beyond what an official sanctions or court
  publication states.
- **Confidence:** official, entity-owned and ledger-derived data → high; research datasets
  → medium; web crawls and single untrusted reports → low. Labels are never "certain".
- **Use in traces:** see `docs/TRACING.md` ("Labels during a trace").

## Consequences
- Every label shown can be checked by following its source link.
- Coverage is thinner than commercial tools, especially for recent exchange deposit
  wallets; behavioural detection ("likely service") covers the gap, and same-wallet naming
  extends each known address to the transactions it appears in.
- `data/labels/SOURCES.md` documents what was imported and what was rejected, and why.
