//! Trace configuration and results.

use ct_core::{Address, Claim, Confidence, EntityCategory};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Where did the funds go?
    Forward,
    /// Where did the funds come from?
    Backward,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConfig {
    pub direction: Direction,
    /// Maximum transactions between the seed and a lead.
    pub max_hops: u32,
    /// Maximum distinct transactions fetched (keeps free APIs happy and runs bounded).
    pub max_txs: u32,
    /// Branches smaller than this (sats) are not followed. `None`: 0.1% of the seed amount,
    /// at least 1,000 sats.
    pub min_amount: Option<u64>,
    /// Look up activity of addresses the funds reach, to detect busy service wallets.
    pub check_addresses: bool,
    /// Stop at services detected with at least this confidence.
    pub stop_confidence: Confidence,
    /// Continue an earlier trace: follow only this many sats of the seed output(s), the share
    /// that came from the earlier trace (split pro-rata over several seed outputs).
    #[serde(default)]
    pub seed_share: Option<u64>,
    /// Continue an earlier trace whose funds were already mixed in this transaction: the new
    /// trace's amounts are allocations from the start.
    #[serde(default)]
    pub mixed_in: Option<String>,
}

impl Default for TraceConfig {
    fn default() -> Self {
        TraceConfig {
            direction: Direction::Forward,
            max_hops: 8,
            max_txs: 100,
            min_amount: None,
            check_addresses: true,
            stop_confidence: Confidence::Medium,
            seed_share: None,
            mixed_in: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub enum StopReason {
    HopLimit,
    Budget,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LeadKind {
    /// Funds reached a labelled or behaviourally detected service, mixer, etc.
    Endpoint,
    /// Funds are sitting unspent.
    AtRest,
    /// Funds were newly mined (backward traces).
    Mined,
    /// Tracing stopped before reaching an endpoint.
    Unexplored { stop: StopReason },
}

/// Where traced funds ended up (forward) or came from (backward).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lead {
    pub kind: LeadKind,
    /// One-line description, e.g. "Likely service: deposit swept with 70 other addresses".
    pub title: String,
    pub category: Option<EntityCategory>,
    pub address: Option<Address>,
    /// The traced address next to this lead on the path: where the funds were just before
    /// reaching it (forward) or just after leaving it (backward). Used to draw the graph.
    #[serde(default)]
    pub via: Option<Address>,
    /// Transaction where the lead was found. For funds at rest and branches not followed:
    /// the transaction that created the coin (`txid:vout`); for endpoints found by an
    /// address: the transaction that paid it; for endpoints found by a transaction's shape
    /// (CoinJoin, service sweep or payout): that transaction.
    pub txid: Option<String>,
    /// Output of `txid` holding the funds, when the lead is one coin (at rest, not followed).
    #[serde(default)]
    pub vout: Option<u32>,
    /// Transaction where the traced funds on the way here were first mixed with other funds.
    /// From there on amounts are a pro-rata share (an allocation under the rule recorded with
    /// the trace, not an on-chain fact). `None`: the amount moved here unmixed.
    #[serde(default)]
    pub mixed_in: Option<String>,
    /// Traced amount reaching this lead, in base units.
    pub amount: u64,
    /// Block time (Unix seconds) when funds first reached it.
    pub first_time: Option<u64>,
    /// Fewest hops from the seed.
    pub hops: u32,
    /// Weakest of the path and the lead's own claim.
    pub confidence: Confidence,
    /// Why we believe what the lead says.
    pub claim: Claim,
    /// Transactions from the seed to the lead (first path found).
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum FlowRole {
    /// Believed to stay with the same owner (change output).
    Change { confidence: Confidence },
    /// Went to another party, as far as we can tell.
    Payment,
    /// Backward step: an input that funded the transaction.
    Source,
}

/// One hop of traced value: an edge for the graph view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub txid: String,
    pub hop: u32,
    pub time: Option<u64>,
    pub from: Option<Address>,
    pub to: Option<Address>,
    /// The coins (`txid:vout`) the funds moved from and to, so outputs without an address
    /// (P2PK, bare multisig) can be followed from one transfer to the next.
    #[serde(default)]
    pub from_coin: Option<String>,
    #[serde(default)]
    pub to_coin: Option<String>,
    // A coin reference without ":vout" stands for a whole transaction (e.g. funds entering a
    // CoinJoin, or leaving a service's payout): the graph draws it as the transaction.
    /// Transaction where the traced funds on this transfer were first mixed with others (its
    /// amount is then a pro-rata share). `None`: unmixed.
    #[serde(default)]
    pub mixed_in: Option<String>,
    /// Traced amount on this edge (not the full output value), base units.
    pub amount: u64,
    pub role: FlowRole,
    /// The transaction is a peel-chain step.
    pub peel: bool,
    /// Shape of the transaction when notable (e.g. "batch payout to 24 addresses").
    pub note: Option<String>,
}

/// A labelled address the funds passed through without the trace stopping there (label
/// below the stop confidence, or a category that is not an endpoint).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSeen {
    pub address: Address,
    pub entity: String,
    pub category: EntityCategory,
    pub claim: Claim,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceStats {
    pub txs_examined: u32,
    pub addresses_checked: u32,
    /// Traced value spent on fees (forward) — includes rounding.
    pub fees: u64,
    /// Traced value in branches below the minimum amount.
    pub below_threshold: u64,
    pub peel_steps: u32,
    pub budget_exhausted: bool,
}

/// One output of the seed transaction included in the trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedOutput {
    pub txid: String,
    pub vout: u32,
    pub address: Option<Address>,
    pub amount: u64,
    pub time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceReport {
    pub direction: Direction,
    pub seed: String,
    pub seed_amount: u64,
    /// The outputs the trace started from (forward) or whose funding it traced (backward).
    #[serde(default)]
    pub seed_outputs: Vec<SeedOutput>,
    /// Allocation rule for mixed funds (`docs/TRACING.md` §5).
    pub allocation: String,
    pub min_amount: u64,
    pub config: TraceConfig,
    /// Sorted by amount, largest first.
    pub leads: Vec<Lead>,
    pub flows: Vec<Flow>,
    /// Labelled addresses passed through (not endpoints).
    pub labels_seen: Vec<LabelSeen>,
    pub stats: TraceStats,
}

impl TraceReport {
    /// Amount accounted for by leads, fees and dust. Equals `seed_amount` by construction.
    pub fn accounted(&self) -> u64 {
        self.leads.iter().map(|l| l.amount).sum::<u64>()
            + self.stats.fees
            + self.stats.below_threshold
    }
}

/// Progress snapshot for UIs.
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub txs_examined: u32,
    pub pending: usize,
    pub leads: usize,
}
