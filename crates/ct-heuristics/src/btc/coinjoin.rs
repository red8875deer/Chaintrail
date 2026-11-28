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
    /// Many addresses' coins merged into equal "chips" of a power-of-two multiple of
    /// 0.001 BTC with no change (how ChipMixer prepared its chips).
    ChipSplit,
}

impl CoinJoinKind {
    pub fn label(self) -> &'static str {
        match self {
            CoinJoinKind::Whirlpool => "Whirlpool-style CoinJoin",
            CoinJoinKind::WabiSabi => "WabiSabi CoinJoin (Wasabi-style)",
            CoinJoinKind::Generic => "CoinJoin (equal-output pattern)",
            CoinJoinKind::ChipSplit => "chip split (ChipMixer-style)",
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

/// Generic CoinJoin: at most this many inputs per equal output. Each participant brings a
/// few inputs; hundreds of inputs merged into a dozen equal outputs are one owner.
pub const GENERIC_MAX_INPUTS_PER_EQUAL: usize = 6;
/// Chip split: at least this many equal chips.
pub const CHIP_MIN_COUNT: usize = 5;

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
    // In a CoinJoin every participant gets their own outputs: equal outputs all paid to one
    // address are one owner splitting or consolidating, not mixing.
    let key = |o: &ct_core::TxOutput| {
        o.address
            .as_ref()
            .map(|a| a.as_str().to_string())
            .unwrap_or_else(|| format!("#{}", o.index))
    };
    let distinct_equal = spendable_outputs(tx)
        .filter(|o| o.value == equal_value)
        .map(key)
        .collect::<HashSet<_>>()
        .len();
    let distinct_out = spendable_outputs(tx).map(key).collect::<HashSet<_>>().len();
    let separate_owners = |distinct: usize, count: usize| distinct * 10 >= count * 9;

    // Whirlpool: N inputs, N outputs, all outputs identical, N in 5..=8.
    if (5..=8).contains(&n_in)
        && n_in == n_out
        && equal_count == n_out
        && distinct_inputs >= 2
        && distinct_equal == equal_count
    {
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
        if share >= WABISABI_MIN_STD_SHARE
            && distinct_inputs >= WABISABI_MIN_IO / 2
            && separate_owners(distinct_out, n_out)
        {
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

    // Chip split: every output is one chip value (2^k × 0.001 BTC) except at most one small
    // remainder; inputs from several addresses. A pattern, not proof: Medium.
    let remainders = outputs.iter().filter(|v| **v != equal_value).count();
    if equal_count >= CHIP_MIN_COUNT
        && is_chip_value(equal_value)
        && remainders <= 1
        && outputs
            .iter()
            .all(|v| *v == equal_value || *v < equal_value / 10)
        && distinct_inputs >= 2
        && separate_owners(distinct_equal, equal_count)
    {
        return Some(CoinJoinInfo {
            kind: CoinJoinKind::ChipSplit,
            equal_value,
            equal_count,
            confidence: Confidence::Medium,
            evidence: vec![
                format!(
                    "{n_in} inputs from {distinct_inputs} addresses merged into {equal_count} outputs of exactly {equal_value} sats ({} × 0.001 BTC)",
                    equal_value / 100_000
                ),
                "no change output: chip-sized outputs as prepared by mixing services such as ChipMixer".into(),
            ],
        });
    }

    // Generic equal-output CoinJoin (e.g. JoinMarket): at least 3 equal outputs, at least
    // as many inputs as equal outputs (each participant brings an input) but not hundreds
    // per output, inputs from several addresses, and no more than one change output per
    // participant.
    // Outputs at a repeated value: Wasabi 1.x rounds pay several mixing levels (v, ~2v,
    // ~4v…), each a group of equal outputs; the rest is change.
    let levels: Vec<(u64, usize)> = {
        let mut l: Vec<_> = counts
            .iter()
            .filter(|(_, n)| **n >= 2)
            .map(|(v, n)| (*v, *n))
            .collect();
        l.sort_by_key(|(v, n)| (std::cmp::Reverse(*n), *v));
        l
    };
    let repeated: usize = levels.iter().map(|(_, n)| n).sum();
    if equal_count >= 3
        && n_in >= equal_count
        && n_in <= GENERIC_MAX_INPUTS_PER_EQUAL * equal_count
        && distinct_inputs >= 2
        && n_out <= 2 * repeated + 1
        && equal_value > 0
        && distinct_equal >= 3
        && separate_owners(distinct_equal, equal_count)
    {
        let confidence = if equal_count >= 5 {
            Confidence::High
        } else {
            Confidence::Medium
        };
        let mut evidence = vec![
            format!("{equal_count} outputs of exactly {equal_value} sats"),
            format!("{n_in} inputs from {distinct_inputs} addresses, {n_out} outputs"),
        ];
        if levels.len() > 1 {
            evidence.push(format!(
                "{} groups of equal outputs ({}), {} other outputs (change)",
                levels.len(),
                levels
                    .iter()
                    .take(5)
                    .map(|(v, n)| format!("{n} × {v} sats"))
                    .collect::<Vec<_>>()
                    .join(", "),
                n_out - repeated
            ));
        }
        return Some(CoinJoinInfo {
            kind: CoinJoinKind::Generic,
            equal_value,
            equal_count,
            confidence,
            evidence,
        });
    }
    None
}

/// A participant's change in an equal-output CoinJoin, matched by amount: an input of value
/// v, the round's equal amount e and exactly one output c with c ≈ v − e (JoinMarket makers
/// receive a small fee, takers pay fees). Only for rounds with a single equal amount, where
/// the arithmetic identifies the change; the equal outputs themselves stay unlinkable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangePair {
    /// Index into `tx.inputs`.
    pub input: usize,
    /// Output index (`TxOutput::index`).
    pub output: u32,
    /// c − (v − e), sats: positive for a maker's fee, negative for fees paid.
    pub delta: i64,
}

/// Change outputs may differ from v − e by at most this share of e (fees), or 10,000 sats.
pub const CHANGE_MATCH_TOLERANCE_PCT: u64 = 1;

pub fn change_pairs(tx: &UtxoTx, info: &CoinJoinInfo) -> Vec<ChangePair> {
    if info.kind != CoinJoinKind::Generic {
        return Vec::new();
    }
    let outputs: Vec<_> = spendable_outputs(tx).collect();
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for o in &outputs {
        *counts.entry(o.value).or_default() += 1;
    }
    // One mixed amount only (JoinMarket); several levels (Wasabi 1.x) make it ambiguous.
    if counts.values().filter(|n| **n >= 2).count() != 1 {
        return Vec::new();
    }
    let e = info.equal_value;
    let tol = (e * CHANGE_MATCH_TOLERANCE_PCT / 100).max(10_000) as i64;
    let candidates: Vec<_> = outputs.iter().filter(|o| counts[&o.value] == 1).collect();
    let mut pairs: Vec<ChangePair> = Vec::new();
    for (k, input) in tx.inputs.iter().enumerate() {
        if input.value <= e {
            continue;
        }
        let want = (input.value - e) as i64;
        let hits: Vec<_> = candidates
            .iter()
            .filter(|o| (o.value as i64 - want).abs() <= tol)
            .collect();
        if let [o] = hits.as_slice() {
            pairs.push(ChangePair {
                input: k,
                output: o.index,
                delta: o.value as i64 - want,
            });
        }
    }
    // An output matched by two inputs is not identified.
    let mut used: HashMap<u32, usize> = HashMap::new();
    for p in &pairs {
        *used.entry(p.output).or_default() += 1;
    }
    pairs.retain(|p| used[&p.output] == 1);
    pairs
}

/// ChipMixer-style chip values: 0.001 BTC times a power of two, up to 8.192 BTC.
pub fn is_chip_value(v: u64) -> bool {
    v % 100_000 == 0 && (v / 100_000).is_power_of_two() && v / 100_000 <= 8_192
}

/// Whirlpool pool denominations (sats).
pub const WHIRLPOOL_POOLS: [u64; 4] = [100_000, 1_000_000, 5_000_000, 50_000_000];

/// A Whirlpool Tx0: one wallet splits coins into "premix" outputs for one pool (the pool
/// amount plus a mining-fee allowance), pays the coordinator fee and keeps change, with an
/// OP_RETURN. The premix outputs can only go on into Whirlpool mixes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tx0Info {
    /// Pool denomination (sats).
    pub pool: u64,
    /// Value of each premix output (sats).
    pub premix_value: u64,
    pub premix_count: usize,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
}

/// Premix outputs are at most this much above the pool amount (fee allowance).
const TX0_MAX_PREMIX_OVER_POOL_PCT: u64 = 5;
/// Outputs besides premix and OP_RETURN (coordinator fee, change).
const TX0_MAX_OTHER_OUTPUTS: usize = 3;

pub fn detect_whirlpool_tx0(tx: &UtxoTx) -> Option<Tx0Info> {
    if tx.is_coinbase() || tx.inputs.is_empty() {
        return None;
    }
    let op_returns = tx
        .outputs
        .iter()
        .filter(|o| !o.script_type.is_spendable())
        .count();
    if op_returns != 1 {
        return None;
    }
    let outputs: Vec<_> = spendable_outputs(tx).collect();
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for o in &outputs {
        *counts.entry(o.value).or_default() += 1;
    }
    let (premix_value, premix_count) = counts
        .iter()
        .max_by_key(|(v, n)| (**n, **v))
        .map(|(v, n)| (*v, *n))?;
    if premix_count < 2 || outputs.len() - premix_count > TX0_MAX_OTHER_OUTPUTS {
        return None;
    }
    let pool = WHIRLPOOL_POOLS.into_iter().find(|p| {
        premix_value >= *p && premix_value <= p + p * TX0_MAX_PREMIX_OVER_POOL_PCT / 100
    })?;
    let distinct = outputs
        .iter()
        .filter(|o| o.value == premix_value)
        .filter_map(|o| o.address.as_ref())
        .collect::<HashSet<_>>()
        .len();
    if distinct != premix_count {
        return None;
    }
    Some(Tx0Info {
        pool,
        premix_value,
        premix_count,
        confidence: Confidence::High,
        evidence: vec![
            format!(
                "{premix_count} outputs of exactly {premix_value} sats: premix for the {} BTC Whirlpool pool",
                ct_core::Amount::new(pool as u128, 8)
            ),
            format!(
                "an OP_RETURN and {} other output(s) (coordinator fee, change), as in a Whirlpool Tx0",
                outputs.len() - premix_count
            ),
        ],
    })
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
    fn equal_outputs_to_one_address_are_not_a_coinjoin() {
        // WannaCry-era consolidation: 727 inputs into 6 x 500 BTC + 2 more, all paid to one
        // address. Equal values, but one owner: not mixing.
        assert!(detect_coinjoin(&fixtures::tx("consolidation_equal_outputs")).is_none());
    }

    #[test]
    fn exchange_patterns_are_not_coinjoins() {
        // Batch payout: 2 inputs, 156 outputs including 4 equal ones.
        assert!(detect_coinjoin(&fixtures::tx("batch_payout_156out")).is_none());
        // Consolidation: 85 inputs, 1 output.
        assert!(detect_coinjoin(&fixtures::tx("consolidation_85in")).is_none());
    }

    #[test]
    fn wasabi_1_round_with_mixing_levels() {
        // 82 inputs, 161 outputs: 67 × 0.1004 BTC, 25 × ~2x, 13 × ~4x, 6 × ~8x, 2 × ~16x,
        // and 48 change outputs.
        let info = detect_coinjoin(&fixtures::tx("coinjoin_wasabi1_161out")).unwrap();
        assert_eq!(info.kind, CoinJoinKind::Generic);
        assert_eq!(info.confidence, Confidence::High);
        assert_eq!(info.equal_count, 67);
        assert!(
            info.evidence.iter().any(|e| e.contains("5 groups")),
            "{:?}",
            info.evidence
        );
    }

    #[test]
    fn postmix_merge_is_not_a_coinjoin() {
        // 278 Whirlpool postmix coins (0.05 BTC each) merged into 12 × 1 BTC + 4 × 0.475:
        // one owner, not hundreds of inputs per participant.
        assert!(detect_coinjoin(&fixtures::tx("postmix_merge_278in")).is_none());
    }

    #[test]
    fn chip_split_is_named_as_such() {
        // 42 inputs into 30 × 1.024 BTC (1024 × 0.001) and 2,339 sats: chips, not CoinJoin.
        let info = detect_coinjoin(&fixtures::tx("chip_split_42in")).unwrap();
        assert_eq!(info.kind, CoinJoinKind::ChipSplit);
        assert_eq!(info.confidence, Confidence::Medium);
        assert_eq!(info.equal_value, 102_400_000);
        assert!(is_chip_value(100_000) && is_chip_value(819_200_000));
        assert!(!is_chip_value(46_400_000) && !is_chip_value(1_638_400_000));
    }

    #[test]
    fn whirlpool_tx0() {
        let tx = fixtures::tx("whirlpool_tx0_244premix");
        let info = detect_whirlpool_tx0(&tx).unwrap();
        assert_eq!(info.pool, 5_000_000);
        assert_eq!(info.premix_value, 5_010_000);
        assert_eq!(info.premix_count, 244);
        // One input: not a CoinJoin.
        assert!(detect_coinjoin(&tx).is_none());
        for name in [
            "batch_payout_156out",
            "coinjoin_wabisabi",
            "consolidation_85in",
            "change_type_and_round",
            "chip_split_42in",
        ] {
            assert!(
                detect_whirlpool_tx0(&fixtures::tx(name)).is_none(),
                "{name}"
            );
        }
    }

    #[test]
    fn joinmarket_change_is_matched_by_amount() {
        // Real JoinMarket round: 12 inputs, 11 × 6,357,366 sats. Maker input 1 (20.96 BTC)
        // has its change in output 6: input − equal output + a 413-sat maker fee.
        let tx = fixtures::tx("coinjoin_joinmarket");
        let info = detect_coinjoin(&tx).unwrap();
        assert_eq!(info.equal_value, 6_357_366);
        let pairs = change_pairs(&tx, &info);
        let p = pairs.iter().find(|p| p.input == 1).expect("maker change");
        assert_eq!((p.output, p.delta), (6, 413));
        assert!(pairs.len() >= 6, "{pairs:?}");
        // Several mixing levels (Wasabi 1.x): change is not matched.
        let w = fixtures::tx("coinjoin_wasabi1_161out");
        assert!(change_pairs(&w, &detect_coinjoin(&w).unwrap()).is_empty());
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
