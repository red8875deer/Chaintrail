//! Transaction shape: what kind of activity a transaction looks like.
//!
//! Shapes decide where tracing stops. A consolidation of many deposit addresses or a large
//! batch payout is how custodial services (exchanges, payment processors, pools) move
//! customer funds, so funds reaching one have reached a service.

use std::collections::HashSet;

use ct_core::{Confidence, UtxoTx};
use serde::{Deserialize, Serialize};

use super::change::ChangeGuess;
use super::coinjoin::{CoinJoinInfo, Tx0Info, detect_coinjoin, detect_whirlpool_tx0};
use super::spendable_outputs;

/// Distinct input addresses for a consolidation to suggest a service sweep (Medium).
pub const CONSOLIDATION_SERVICE_INPUTS: usize = 50;
/// Distinct input addresses for a consolidation to be reported at all (Low hint).
pub const CONSOLIDATION_MIN_INPUTS: usize = 20;
/// Distinct output addresses for a batch payout to be reported at all (Low hint).
/// Launderers also fan out to a few dozen mule addresses, so small batches do not stop a
/// trace.
pub const BATCH_MIN_OUTPUTS: usize = 20;
/// Distinct output addresses for a batch payout to suggest a service (Medium).
pub const BATCH_SERVICE_OUTPUTS: usize = 50;
/// Distinct output addresses for a batch payout to be High.
pub const BATCH_HIGH_OUTPUTS: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum TxShape {
    /// Newly mined coins.
    Coinbase,
    CoinJoin(CoinJoinInfo),
    /// One wallet preparing coins for Whirlpool: its premix outputs go on into mixes; its
    /// change stays with the wallet.
    WhirlpoolTx0(Tx0Info),
    /// Many inputs swept into one or two outputs.
    Consolidation {
        distinct_inputs: usize,
        confidence: Confidence,
    },
    /// One sender paying many recipients at once (typical of exchange withdrawals).
    BatchPayout {
        distinct_outputs: usize,
        confidence: Confidence,
    },
    /// An ordinary transaction.
    Simple,
}

impl TxShape {
    /// Whether this shape means the funds involved belong to a service, with the confidence
    /// to report. `Low` hints are reported but tracing continues.
    pub fn service_confidence(&self) -> Option<Confidence> {
        match self {
            TxShape::Consolidation { confidence, .. } | TxShape::BatchPayout { confidence, .. } => {
                Some(*confidence)
            }
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            TxShape::Coinbase => "coinbase (newly mined coins)".into(),
            TxShape::CoinJoin(info) => format!(
                "{} ({} outputs of {} sats)",
                info.kind.label(),
                info.equal_count,
                info.equal_value
            ),
            TxShape::WhirlpoolTx0(info) => format!(
                "Whirlpool Tx0 ({} premix outputs of {} sats)",
                info.premix_count, info.premix_value
            ),
            TxShape::Consolidation {
                distinct_inputs, ..
            } => format!("consolidation of {distinct_inputs} addresses"),
            TxShape::BatchPayout {
                distinct_outputs, ..
            } => format!("batch payout to {distinct_outputs} addresses"),
            TxShape::Simple => "ordinary transaction".into(),
        }
    }
}

pub fn classify(tx: &UtxoTx) -> TxShape {
    if tx.is_coinbase() {
        return TxShape::Coinbase;
    }
    if let Some(info) = detect_whirlpool_tx0(tx) {
        return TxShape::WhirlpoolTx0(info);
    }
    if let Some(info) = detect_coinjoin(tx) {
        return TxShape::CoinJoin(info);
    }
    let distinct_inputs = tx
        .inputs
        .iter()
        .filter_map(|i| i.address.as_ref())
        .collect::<HashSet<_>>()
        .len();
    let outputs: Vec<_> = spendable_outputs(tx).collect();
    let distinct_outputs = outputs
        .iter()
        .filter_map(|o| o.address.as_ref())
        .collect::<HashSet<_>>()
        .len();
    // Recipients: distinct addresses, each output without an address counting on its own.
    // Several outputs to one address are still one recipient (a consolidation split up).
    let recipients = distinct_outputs + outputs.iter().filter(|o| o.address.is_none()).count();

    if recipients <= 2 && distinct_inputs >= CONSOLIDATION_MIN_INPUTS {
        let confidence = if distinct_inputs >= CONSOLIDATION_SERVICE_INPUTS {
            Confidence::Medium
        } else {
            Confidence::Low
        };
        return TxShape::Consolidation {
            distinct_inputs,
            confidence,
        };
    }
    if distinct_outputs >= BATCH_MIN_OUTPUTS && tx.inputs.len() < distinct_outputs {
        let confidence = if distinct_outputs >= BATCH_HIGH_OUTPUTS {
            Confidence::High
        } else if distinct_outputs >= BATCH_SERVICE_OUTPUTS {
            Confidence::Medium
        } else {
            Confidence::Low
        };
        return TxShape::BatchPayout {
            distinct_outputs,
            confidence,
        };
    }
    TxShape::Simple
}

/// A peel-chain step (`docs/TRACING.md` §3.4): few inputs, two outputs, a confident change
/// output at least five times larger than the payment.
pub fn is_peel_step(tx: &UtxoTx, change: Option<&ChangeGuess>) -> bool {
    let Some(change) = change.filter(|c| c.confidence.followable()) else {
        return false;
    };
    let outputs: Vec<_> = spendable_outputs(tx).collect();
    if tx.inputs.len() > 2 || outputs.len() != 2 {
        return false;
    }
    let (Some(c), Some(p)) = (
        outputs.iter().find(|o| o.index == change.output),
        outputs.iter().find(|o| o.index != change.output),
    ) else {
        return false;
    };
    c.value >= p.value.saturating_mul(5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::change::detect_change;
    use crate::btc::fixtures;

    #[test]
    fn several_outputs_to_one_address_are_a_consolidation() {
        let tx = fixtures::tx("consolidation_equal_outputs");
        match classify(&tx) {
            TxShape::Consolidation {
                distinct_inputs,
                confidence,
            } => {
                assert!(distinct_inputs >= 400, "{distinct_inputs}");
                assert_eq!(confidence, Confidence::Medium);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn real_shapes() {
        assert!(matches!(
            classify(&fixtures::tx("coinjoin_wabisabi")),
            TxShape::CoinJoin(_)
        ));
        match classify(&fixtures::tx("consolidation_85in")) {
            TxShape::Consolidation {
                distinct_inputs,
                confidence,
            } => {
                assert_eq!(distinct_inputs, 71);
                assert_eq!(confidence, Confidence::Medium);
            }
            other => panic!("{other:?}"),
        }
        match classify(&fixtures::tx("batch_payout_156out")) {
            TxShape::BatchPayout {
                distinct_outputs,
                confidence,
            } => {
                assert!(distinct_outputs >= 150);
                assert_eq!(confidence, Confidence::High);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            classify(&fixtures::tx("change_round_only")),
            TxShape::Simple
        );
        // A Tx0 with 247 outputs is not a service batch payout.
        assert!(matches!(
            classify(&fixtures::tx("whirlpool_tx0_244premix")),
            TxShape::WhirlpoolTx0(_)
        ));
    }

    #[test]
    fn peel_step_requires_large_confident_change() {
        // 0.2 BTC payment, 0.38 BTC change: change is not 5x larger.
        let tx = fixtures::tx("change_type_and_round");
        assert!(!is_peel_step(&tx, detect_change(&tx).as_ref()));
        // Address reuse: 0.00132661 payment, 1.279 BTC change → peel step.
        let tx = fixtures::tx("change_address_reuse");
        assert!(is_peel_step(&tx, detect_change(&tx).as_ref()));
    }
}
