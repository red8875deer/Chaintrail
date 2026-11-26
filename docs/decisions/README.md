# Architecture decision records

One file per significant decision: context, decision, consequences.
Supersede a decision with a new record instead of editing the old one.

| # | Decision | Status |
|---|---|---|
| [0001](0001-standalone-zero-cost.md) | Standalone desktop app with zero running cost | Accepted |
| [0002](0002-rust-tauri-react.md) | Rust core, Tauri 2 shell, React + TypeScript UI | Accepted |
| [0003](0003-initial-coverage.md) | First chains: Bitcoin, Ethereum, Tron | Accepted |
| [0004](0004-on-demand-analysis.md) | On-demand fetching and local analysis | Accepted |
| [0005](0005-claims-model.md) | Facts, inferences and attributions are separate claim kinds | Accepted |
| [0006](0006-license.md) | Apache-2.0 licence | Accepted (revisit before outside contributions) |
| [0007](0007-dependency-licences.md) | Permissive dependency licences only | Accepted |
| [0008](0008-label-data.md) | Label data sources and format | Accepted |
| [0009](0009-mpl-in-desktop-shell.md) | MPL-2.0 allowed for unmodified desktop-shell dependencies | Accepted |
| [0010](0010-app-architecture.md) | One service, two front doors (desktop app and local browser mode) | Accepted |
| [0011](0011-graph-display-layer.md) | The drawn graph is a readable view of the trace, never a different trace | Accepted |
