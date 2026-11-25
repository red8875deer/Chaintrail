//! Account-model value movements (Ethereum, Tron).

use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::amount::Amount;
use crate::asset::Asset;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    /// Top-level native transfer (the transaction's `value`).
    Native,
    /// Native transfer made by a contract during execution (trace / internal tx).
    Internal,
    /// Token transfer event (ERC-20 / TRC-20 `Transfer`).
    Token,
}

/// One movement of value between two accounts. Always an on-chain fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Transfer {
    pub tx_hash: String,
    /// Position within the transaction (log index or trace index) so multiple transfers in
    /// one transaction stay distinct and ordered.
    pub index: u32,
    pub block_height: u64,
    /// Unix seconds.
    pub time: u64,
    pub from: Address,
    pub to: Address,
    pub asset: Asset,
    pub amount: Amount,
    pub kind: TransferKind,
}
