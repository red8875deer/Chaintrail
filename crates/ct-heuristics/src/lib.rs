//! Heuristics: pure functions that turn on-chain facts into scored inferences.
//!
//! Every heuristic returns its confidence and human-readable evidence, and never claims
//! certainty (`docs/TRACING.md`, ADR-0005). Weights are initial values to be calibrated
//! against labelled data; change them only together with the tests and the docs.

pub mod btc;
