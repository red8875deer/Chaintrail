//! CoinJoin detection (`docs/TRACING.md` §3.3).
//!
//! A CoinJoin merges coins from many people into outputs of equal value so outputs cannot
//! be linked to inputs. Detecting them matters twice: tracing stops there (mixer endpoint),
//! and common-input clustering must never be applied to them.

use std::collections::{HashMap, HashSet};

use ct_core::{Confidence, UtxoTx};
use serde::{Deserialize, Serialize};

use super::spendable_outputs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoinJoinKind {
    /// Whirlpool-style: 5–8 inputs and the same number of identical outputs.
    Whirlpool,
    /// WabiSabi coordinators (Wasabi 2.x and compatible): hundreds of inputs and outputs
    /// using standard denominations.
    WabiSabi,
    /// JoinMarket-style or other: several equal outputs plus change, inputs from several
    /// owners.
    Generic,
}

impl CoinJoinKind {
    pub fn label(self) -> &'static str {
        match self {
            CoinJoinKind::Whirlpool => "Whirlpool-style CoinJoin",
            CoinJoinKind::WabiSabi => "WabiSabi CoinJoin (Wasabi-style)",
            CoinJoinKind::Generic => "CoinJoin (equal-output pattern)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoinJoinInfo {
    pub kind: CoinJoinKind,
    /// The most common output value, in sats.
    pub equal_value: u64,
    /// How many outputs have that value.
    pub equal_count: usize,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
}

/// Minimum inputs and outputs for a WabiSabi round.
const WABISABI_MIN_IO: usize = 50;
/// Share of outputs that must use standard denominations for a WabiSabi round.
const WABISABI_MIN_STD_SHARE: f64 = 0.5;

pub fn detect_coinjoin(tx: &UtxoTx) -> Option<CoinJoinInfo> {
    if tx.is_coinbase() || tx.inputs.len() < 2 {
        return None;
    }
    let outputs: Vec<u64> = spendable_outputs(tx).map(|o| o.value).collect();
    if outputs.len() < 2 {
        return None;
    }
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for v in &outputs {
        *counts.entry(*v).or_default() += 1;
    }
    // Most common value; ties broken by larger value for determinism.
    let (equal_value, equal_count) = counts
        .iter()
        .max_by_key(|(v, n)| (**n, **v))
        .map(|(v, n)| (*v, *n))?;
    let distinct_inputs = tx
        .inputs
        .iter()
        .map(|i| {
            i.address
                .as_ref()
                .map(|a| a.as_str().to_string())
                .unwrap_or_else(|| format!("{:?}:{}", i.prev_txid, i.prev_vout))
        })
        .collect::<HashSet<_>>()
        .len();
    let n_in = tx.inputs.len();
    let n_out = outputs.len();

    // Whirlpool: N inputs, N outputs, all outputs identical, N in 5..=8.
    if (5..=8).contains(&n_in) && n_in == n_out && equal_count == n_out && distinct_inputs >= 2 {
        return Some(CoinJoinInfo {
            kind: CoinJoinKind::Whirlpool,
            equal_value,
            equal_count,
            confidence: Confidence::High,
            evidence: vec![format!(
                "{n_in} inputs and {n_out} outputs of exactly {equal_value} sats each"
            )],
        });
    }

    // WabiSabi: very large round with standard denominations.
    if n_in >= WABISABI_MIN_IO && n_out >= WABISABI_MIN_IO {
        let std_count = outputs
            .iter()
            .filter(|v| is_standard_denomination(**v))
            .count();
        let share = std_count as f64 / n_out as f64;
        if share >= WABISABI_MIN_STD_SHARE && distinct_inputs >= WABISABI_MIN_IO / 2 {
            return Some(CoinJoinInfo {
                kind: CoinJoinKind::WabiSabi,
                equal_value,
                equal_count,
                confidence: Confidence::High,
                evidence: vec![
                    format!("{n_in} inputs from {distinct_inputs} addresses, {n_out} outputs"),
                    format!("{std_count} of {n_out} outputs use standard CoinJoin denominations"),
                ],
            });
        }
    }

    // Generic equal-output CoinJoin (e.g. JoinMarket): at least 3 equal outputs, at least
    // as many inputs as equal outputs (each participant brings an input), inputs from
    // several addresses, and no more than one change output per participant.
    if equal_count >= 3
        && n_in >= equal_count
        && distinct_inputs >= 2
        && n_out <= 2 * equal_count + 1
        && equal_value > 0
    {
        let confidence = if equal_count >= 5 {
            Confidence::High
        } else {
            Confidence::Medium
        };
        return Some(CoinJoinInfo {
            kind: CoinJoinKind::Generic,
            equal_value,
            equal_count,
            confidence,
            evidence: vec![
                format!("{equal_count} outputs of exactly {equal_value} sats"),
                format!("{n_in} inputs from {distinct_inputs} addresses, {n_out} outputs"),
            ],
        });
    }
    None
}

/// WabiSabi standard denominations: 2^n, 3^n, 2·3^n, 10^n, 2·10^n, 5·10^n sats, from
/// 5,000 sats upwards.
pub fn is_standard_denomination(v: u64) -> bool {
    if v < 5_000 {
        return false;
    }
    let power_of = |mut x: u64, base: u64| {
        while x % base == 0 {
            x /= base;
        }
        x == 1
    };
    v.is_power_of_two()
        || power_of(v, 3)
        || (v % 2 == 0 && power_of(v / 2, 3))
        || power_of(v, 10)
        || (v % 2 == 0 && power_of(v / 2, 10))
        || (v % 5 == 0 && power_of(v / 5, 10))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::fixtures;

    #[test]
    fn standard_denominations() {
        for v in [
            262_144, 354_294, 118_098, 200_000, 5_000, 10_000, 50_000_000, 6_561,
        ] {
            assert!(is_standard_denomination(v), "{v}");
        }
        for v in [4_096 /* below minimum */, 123_456, 300_000, 38_096_090] {
            assert!(!is_standard_denomination(v), "{v}");
        }
    }

    #[test]
    fn real_wabisabi_round() {
        let info = detect_coinjoin(&fixtures::tx("coinjoin_wabisabi")).unwrap();
        assert_eq!(info.kind, CoinJoinKind::WabiSabi);
        assert_eq!(info.confidence, Confidence::High);
        assert_eq!(info.equal_value, 262_144);
        assert_eq!(info.equal_count, 33);
    }

    #[test]
    fn exchange_patterns_are_not_coinjoins() {
        // Batch payout: 2 inputs, 156 outputs including 4 equal ones.
        assert!(detect_coinjoin(&fixtures::tx("batch_payout_156out")).is_none());
        // Consolidation: 85 inputs, 1 output.
        assert!(detect_coinjoin(&fixtures::tx("consolidation_85in")).is_none());
    }

    #[test]
    fn ordinary_payments_are_not_coinjoins() {
        for name in [
            "change_type_and_round",
            "change_address_reuse",
            "change_round_only",
            "change_type_only",
            "change_multi_input",
        ] {
            assert!(detect_coinjoin(&fixtures::tx(name)).is_none(), "{name}");
        }
    }
}
