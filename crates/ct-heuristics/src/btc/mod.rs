//! Bitcoin (UTXO) heuristics. See `docs/TRACING.md` §3.

pub mod change;
pub mod cluster;
pub mod coinjoin;
pub mod dormancy;
pub mod lightning;
pub mod service;
pub mod shape;

pub use change::{ChangeGuess, detect_change};
pub use cluster::{Clusters, MergeEvidence};
pub use coinjoin::{CoinJoinInfo, CoinJoinKind, detect_coinjoin};
pub use dormancy::{Dormancy, dormancy};
pub use service::{ServiceHint, service_from_summary};
pub use shape::{TxShape, classify, is_peel_step};

use ct_core::{TxOutput, UtxoTx};

/// Outputs that can carry value onward (excludes OP_RETURN data outputs).
pub(crate) fn spendable_outputs(tx: &UtxoTx) -> impl Iterator<Item = &TxOutput> {
    tx.outputs.iter().filter(|o| o.script_type.is_spendable())
}

#[cfg(test)]
pub(crate) mod fixtures {
    use ct_core::UtxoTx;

    pub fn tx(name: &str) -> UtxoTx {
        let path = format!(
            "{}/tests/fixtures/btc/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        ct_providers::esplora::parse_tx_json(&json).unwrap()
    }
}
