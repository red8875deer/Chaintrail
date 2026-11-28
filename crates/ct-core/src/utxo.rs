//! UTXO-model transactions (Bitcoin).

use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::amount::Amount;
use crate::chain::Chain;

/// Output script types. Used by change-detection heuristics (`docs/TRACING.md` §3.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptType {
    P2pk,
    P2pkh,
    P2sh,
    P2wpkh,
    P2wsh,
    P2tr,
    Multisig,
    OpReturn,
    Other(String),
}

impl ScriptType {
    /// Whether the output can hold spendable value at an address.
    pub fn is_spendable(&self) -> bool {
        !matches!(self, ScriptType::OpReturn)
    }
}

/// Where and when a transaction was confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockRef {
    pub height: u64,
    pub hash: String,
    /// Unix seconds (block header time).
    pub time: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxInput {
    /// Transaction that created the spent output; `None` for coinbase inputs.
    pub prev_txid: Option<String>,
    pub prev_vout: u32,
    /// Address of the spent output, if it has one (P2PK and bare multisig do not).
    pub address: Option<Address>,
    /// Satoshis.
    pub value: u64,
    pub script_type: Option<ScriptType>,
    pub is_coinbase: bool,
    pub sequence: u32,
    /// For a P2WSH (or P2SH-P2WSH) spend: the witness script, as assembly. Identifies e.g.
    /// 2-of-2 multisig (Lightning channels). `None` when not known (older cached data).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_script: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxOutput {
    pub index: u32,
    pub address: Option<Address>,
    /// Satoshis.
    pub value: u64,
    pub script_type: ScriptType,
    /// Hex script. Identifies outputs without an address (e.g. P2PK).
    pub script_pubkey: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UtxoTx {
    pub chain: Chain,
    pub txid: String,
    /// `None` while unconfirmed.
    pub block: Option<BlockRef>,
    pub version: u32,
    pub locktime: u32,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    /// Satoshis.
    pub fee: u64,
    pub vsize: u32,
}

impl UtxoTx {
    pub fn is_coinbase(&self) -> bool {
        self.inputs.first().is_some_and(|i| i.is_coinbase)
    }

    pub fn total_in(&self) -> u64 {
        self.inputs.iter().map(|i| i.value).sum()
    }

    pub fn total_out(&self) -> u64 {
        self.outputs.iter().map(|o| o.value).sum()
    }

    /// Distinct input addresses, in order of first appearance.
    pub fn input_addresses(&self) -> Vec<&Address> {
        let mut seen: Vec<&Address> = Vec::new();
        for a in self.inputs.iter().filter_map(|i| i.address.as_ref()) {
            if !seen.contains(&a) {
                seen.push(a);
            }
        }
        seen
    }

    pub fn amount(&self, sats: u64) -> Amount {
        Amount::new(sats as u128, self.chain.native_decimals())
    }
}

/// Whether (and by which transaction) an output was spent. Drives forward tracing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputSpend {
    pub spent: bool,
    pub spending_txid: Option<String>,
    pub spending_vin: Option<u32>,
    pub block: Option<BlockRef>,
}

/// Aggregate statistics for an address (confirmed chain only unless noted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressSummary {
    pub address: Address,
    pub tx_count: u64,
    pub received_outputs: u64,
    /// Base units.
    pub received: u128,
    pub spent_outputs: u64,
    /// Base units.
    pub spent: u128,
    pub unconfirmed_tx_count: u64,
    /// Net change from unconfirmed transactions, base units.
    pub unconfirmed_delta: i128,
}

impl AddressSummary {
    /// Confirmed balance in base units.
    pub fn balance(&self) -> u128 {
        self.received.saturating_sub(self.spent)
    }
}
