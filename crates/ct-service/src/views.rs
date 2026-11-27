//! Request and response shapes shared by the desktop app and `chaintrail serve`.
//!
//! These are the UI contract: `app/ui/src/api/types.ts` mirrors them. Amounts are base
//! units (sats) as integers; times are Unix seconds.

use std::collections::BTreeMap;

use ct_core::{AddressSummary, BlockRef, Chain, Confidence, EntityCategory};
use ct_trace::{Direction, GraphView, TraceReport};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct StatusView {
    pub version: String,
    pub provider: String,
    pub cache: Option<String>,
    pub labelled_addresses: usize,
    pub packs: usize,
    pub user_labels_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolveRequest {
    pub query: String,
}

/// What the user pasted.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolveView {
    Address {
        chain: Chain,
        address: String,
        /// Lookups and traces are available for this chain.
        supported: bool,
    },
    Transaction {
        txid: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct LabelView {
    pub entity: String,
    pub category: EntityCategory,
    /// Human-readable category, e.g. "swap service".
    pub category_name: String,
    pub confidence: Confidence,
    pub pack: String,
    /// Where the label can be checked.
    pub source: String,
    pub as_of: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddressRequest {
    pub address: String,
    #[serde(default = "one")]
    pub pages: u32,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize)]
pub struct TxBrief {
    pub txid: String,
    pub height: Option<u64>,
    pub time: Option<u64>,
    /// Net change for the address, sats (negative = sent).
    pub net: i64,
    pub fee: u64,
    pub inputs: usize,
    pub outputs: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddressView {
    pub address: String,
    pub chain: Chain,
    pub summary: AddressSummary,
    /// Confirmed balance, sats.
    pub balance: u64,
    pub labels: Vec<LabelView>,
    pub transactions: Vec<TxBrief>,
    /// More transactions exist than were fetched.
    pub more: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TxRequest {
    pub txid: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeView {
    /// simple | coinbase | coinjoin | consolidation | batch_payout
    pub kind: String,
    pub description: String,
    /// Set when the shape suggests a service wallet.
    pub service_confidence: Option<Confidence>,
    /// Set for CoinJoins.
    pub coinjoin_confidence: Option<Confidence>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeView {
    pub output: u32,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InputView {
    pub address: Option<String>,
    pub value: u64,
    pub script_type: Option<String>,
    pub prev_txid: Option<String>,
    pub prev_vout: u32,
    pub coinbase: bool,
    pub label: Option<LabelView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputView {
    pub index: u32,
    pub address: Option<String>,
    pub value: u64,
    pub script_type: String,
    pub label: Option<LabelView>,
    /// False for data (OP_RETURN) outputs.
    pub spendable: bool,
    pub spent: bool,
    pub spent_by: Option<String>,
    pub spent_height: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TxView {
    pub txid: String,
    pub block: Option<BlockRef>,
    pub fee: u64,
    pub vsize: u32,
    pub total_in: u64,
    pub total_out: u64,
    pub shape: ShapeView,
    pub change: Option<ChangeView>,
    pub peel: bool,
    pub inputs: Vec<InputView>,
    pub outputs: Vec<OutputView>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TraceRequest {
    pub txid: String,
    /// Start from this output only.
    pub vout: Option<u32>,
    /// Start from the outputs paying this address.
    pub to: Option<String>,
    pub direction: Direction,
    pub max_hops: Option<u32>,
    pub max_txs: Option<u32>,
    /// Ignore branches below this many sats (default: 0.1% of the start amount).
    pub min_sats: Option<u64>,
    /// Look up address activity to detect busy service wallets (default true).
    pub check_addresses: Option<bool>,
    /// Continue an earlier trace: follow only this many sats of the seed (its share).
    #[serde(default)]
    pub share_sats: Option<u64>,
    /// Continue an earlier trace whose funds were mixed in this transaction.
    #[serde(default)]
    pub mixed_in: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceView {
    pub report: TraceReport,
    pub graph: GraphView,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LabelLookupRequest {
    pub address: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackView {
    pub id: String,
    pub title: String,
    pub source: String,
    pub licence: String,
    pub as_of: Option<String>,
    pub labels: usize,
    pub origin: String,
    pub category: EntityCategory,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddLabelRequest {
    pub address: String,
    pub entity: String,
    pub category: EntityCategory,
    pub confidence: Confidence,
    pub note: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OfacUpdateView {
    pub as_of: Option<String>,
    pub addresses: usize,
    pub entries: usize,
    pub unsupported: BTreeMap<String, usize>,
    pub reassigned: Vec<String>,
    pub invalid: Vec<String>,
    pub path: String,
}
