# 0005: Facts, inferences and attributions are separate claim kinds

**Status:** Accepted (2026-11-25)

## Context
Tracing mixes certain data (on-chain transfers) with guesses (change outputs, clusters,
bridge matches) and third-party statements (labels). Blurring them produces confident but
wrong conclusions.

## Decision
Every edge, cluster membership and label carries a `Claim`:
- `Fact`: read directly from the chain. Always certain.
- `Inference`: produced by a named heuristic, with evidence and a confidence level.
- `Attribution`: from a named label source, with its confidence and date.

A path's confidence is the minimum of its hops. Low-confidence links are shown but never
followed automatically.

## Consequences
- The UI and reports can always say *why* something is believed.
- Heuristics can be evaluated and tuned individually.
- Slightly more bookkeeping in every data structure.
