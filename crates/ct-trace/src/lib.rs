//! The tracing engine (`docs/TRACING.md`).
//!
//! Follows funds forwards (where did they go?) or backwards (where did they come from?),
//! allocates value pro-rata through each transaction, and stops at endpoints: services,
//! mixers, unspent funds, newly mined coins, or configured limits. The result is a list of
//! leads plus the flows behind them.

pub mod btc;
pub mod graph;
pub mod labels;
pub mod report;
pub mod testing;

pub use btc::{Seed, trace_bitcoin};
pub use graph::{GraphEdge, GraphNode, GraphView, NodeKind};
pub use labels::{LabelLookup, MemoryLabels, NoLabels};
pub use report::{
    Direction, Flow, FlowRole, LabelSeen, Lead, LeadKind, Progress, StopReason, TraceConfig,
    TraceReport, TraceStats,
};
