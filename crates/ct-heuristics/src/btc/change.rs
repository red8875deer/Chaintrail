//! Change detection (`docs/TRACING.md` §3.2): which output returns to the sender?
//!
//! Each signal votes for one output with a weight. Votes for the same output combine as a
//! noisy-OR; votes for different outputs cancel (`final = best × (1 − runner_up)`), so
//! conflicting evidence ends up Low and is not followed automatically.
//!
//! Reference: Kappos et al., "How to Peel a Million", USENIX Security 2022.

use std::collections::HashMap;

use ct_core::{Confidence, ScriptType, UtxoTx};
use serde::{Deserialize, Serialize};

use super::coinjoin::detect_coinjoin;
use super::spendable_outputs;

/// Output goes back to an address that funded the transaction.
const W_ADDRESS_REUSE: f64 = 0.95;
/// Only one output has the same script type as all inputs.
const W_SCRIPT_TYPE: f64 = 0.70;
/// The other outputs are round amounts; this one is not.
const W_ROUND_PAYMENT: f64 = 0.65;
/// Only this output is smaller than every input (a wallet that needed all inputs would
/// leave change smaller than the smallest input).
const W_OPTIMAL_CHANGE: f64 = 0.55;

/// Amounts with at least this many trailing decimal zeros (in sats) count as round.
const ROUND_MIN_ZEROS: u32 = 4;
/// Amounts with at most this many trailing zeros count as not round.
const NOT_ROUND_MAX_ZEROS: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeGuess {
    /// Output index believed to be change.
    pub output: u32,
    /// Combined score in [0, 1] after conflicts.
    pub score: f64,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
}

pub fn detect_change(tx: &UtxoTx) -> Option<ChangeGuess> {
    if tx.is_coinbase() || detect_coinjoin(tx).is_some() {
        return None;
    }
    let outputs: Vec<_> = spendable_outputs(tx).collect();
    if outputs.len() < 2 {
        return None;
    }

    let mut votes: HashMap<u32, Vec<(f64, String)>> = HashMap::new();
    let mut vote = |index: u32, weight: f64, why: String| {
        votes.entry(index).or_default().push((weight, why));
    };

    // 1. Address reuse.
    let input_addrs: Vec<_> = tx
        .inputs
        .iter()
        .filter_map(|i| i.address.as_ref())
        .collect();
    let reused: Vec<_> = outputs
        .iter()
        .filter(|o| o.address.as_ref().is_some_and(|a| input_addrs.contains(&a)))
        .collect();
    if reused.len() == 1 {
        vote(
            reused[0].index,
            W_ADDRESS_REUSE,
            "sends back to an input address (address reuse)".into(),
        );
    }

    // 2. Script type match.
    let input_types: Vec<&ScriptType> = tx
        .inputs
        .iter()
        .filter_map(|i| i.script_type.as_ref())
        .collect();
    if let Some(first) = input_types.first()
        && input_types.len() == tx.inputs.len()
        && input_types.iter().all(|t| t == first)
    {
        let matching: Vec<_> = outputs
            .iter()
            .filter(|o| &o.script_type == *first)
            .collect();
        if matching.len() == 1 {
            vote(
                matching[0].index,
                W_SCRIPT_TYPE,
                format!("only output with the inputs' script type ({first:?})"),
            );
        }
    }

    // 3. Round payment: every other output is round, this one is not.
    let not_round: Vec<_> = outputs
        .iter()
        .filter(|o| trailing_zeros(o.value) <= NOT_ROUND_MAX_ZEROS)
        .collect();
    let round_count = outputs
        .iter()
        .filter(|o| o.value > 0 && trailing_zeros(o.value) >= ROUND_MIN_ZEROS)
        .count();
    if not_round.len() == 1 && round_count == outputs.len() - 1 {
        vote(
            not_round[0].index,
            W_ROUND_PAYMENT,
            "other output(s) are round amounts, this one is not".into(),
        );
    }

    // 4. Optimal change: only this output is smaller than the smallest input.
    if tx.inputs.len() >= 2 {
        let min_in = tx.inputs.iter().map(|i| i.value).min().unwrap_or(0);
        let smaller: Vec<_> = outputs.iter().filter(|o| o.value < min_in).collect();
        if smaller.len() == 1 {
            vote(
                smaller[0].index,
                W_OPTIMAL_CHANGE,
                "only output smaller than every input".into(),
            );
        }
    }

    let mut scored: Vec<(u32, f64, Vec<String>)> = votes
        .into_iter()
        .map(|(index, vs)| {
            let miss: f64 = vs.iter().map(|(w, _)| 1.0 - w).product();
            (
                index,
                1.0 - miss,
                vs.into_iter().map(|(_, why)| why).collect(),
            )
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let (output, best, mut evidence) = scored.first().cloned()?;
    let runner_up = scored.get(1).map(|s| s.1).unwrap_or(0.0);
    let score = best * (1.0 - runner_up);
    if runner_up > 0.0 {
        evidence.push(format!(
            "conflicting signals point to another output (score {runner_up:.2})"
        ));
    }
    Some(ChangeGuess {
        output,
        score,
        confidence: Confidence::from_score(score),
        evidence,
    })
}

fn trailing_zeros(mut v: u64) -> u32 {
    if v == 0 {
        return 0;
    }
    let mut n = 0;
    while v % 10 == 0 {
        v /= 10;
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::fixtures;

    #[test]
    fn script_type_and_round_agree() {
        // 1 P2WPKH input → 0.2 BTC to P2PKH + 0.38096090 BTC to P2WPKH.
        let g = detect_change(&fixtures::tx("change_type_and_round")).unwrap();
        assert_eq!(g.output, 1);
        assert_eq!(g.confidence, Confidence::High);
        assert_eq!(g.evidence.len(), 2);
    }

    #[test]
    fn address_reuse() {
        let g = detect_change(&fixtures::tx("change_address_reuse")).unwrap();
        assert_eq!(g.output, 1);
        assert_eq!(g.confidence, Confidence::High);
        assert!(g.evidence[0].contains("address reuse"));
    }

    #[test]
    fn round_only_is_medium() {
        // Same script types; 0.5 BTC payment, 0.36089071 BTC change.
        let g = detect_change(&fixtures::tx("change_round_only")).unwrap();
        assert_eq!(g.output, 0);
        assert_eq!(g.confidence, Confidence::Medium);
    }

    #[test]
    fn script_type_only_is_medium() {
        let g = detect_change(&fixtures::tx("change_type_only")).unwrap();
        assert_eq!(g.output, 1);
        assert_eq!(g.confidence, Confidence::Medium);
    }

    #[test]
    fn multi_input_round_payment() {
        // 4 inputs → 0.14 BTC + 0.04248415 BTC.
        let g = detect_change(&fixtures::tx("change_multi_input")).unwrap();
        assert_eq!(g.output, 1);
        assert!(g.confidence >= Confidence::Medium);
    }

    #[test]
    fn no_guess_for_coinjoin_or_single_output() {
        assert!(detect_change(&fixtures::tx("coinjoin_wabisabi")).is_none());
        assert!(detect_change(&fixtures::tx("consolidation_85in")).is_none());
    }

    #[test]
    fn conflicting_signals_become_low() {
        // Script type points at output 0, roundness at output 1.
        let mut tx = fixtures::tx("change_type_and_round");
        tx.outputs[0].script_type = ScriptType::P2wpkh;
        tx.outputs[1].script_type = ScriptType::P2pkh;
        // output 0 = 20_000_000 (round), output 1 = 38_096_090 (not round).
        let g = detect_change(&tx).unwrap();
        assert_eq!(g.confidence, Confidence::Low);
        assert!(g.evidence.iter().any(|e| e.contains("conflicting")));
    }

    #[test]
    fn trailing_zero_count() {
        assert_eq!(trailing_zeros(20_000_000), 7);
        assert_eq!(trailing_zeros(38_096_090), 1);
        assert_eq!(trailing_zeros(0), 0);
    }
}
