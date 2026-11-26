//! Long-dormant coins in a consolidation (`docs/TRACING.md` §3.3a).
//!
//! Services sweep customer deposits within hours or weeks. A consolidation whose inputs sat
//! untouched for months or years is an owner gathering long-held coins (for example a thief
//! moving stolen funds years later), not a deposit sweep, so it must not be reported as a
//! service.
//!
//! Measured on real transactions: exchange and darknet-market sweeps had input ages of
//! 0–40 days; the 2016 Bitfinex thief's 2022 consolidations ~2,009 days.

use serde::{Deserialize, Serialize};

/// Inputs older than this are "dormant".
pub const DORMANT_AFTER_DAYS: u64 = 90;
/// How many inputs to sample (the largest by value).
pub const SAMPLE_INPUTS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dormancy {
    pub sampled: usize,
    pub dormant: usize,
    pub median_age_days: u64,
}

impl Dormancy {
    /// Whether most sampled inputs were dormant.
    pub fn mostly_dormant(&self) -> bool {
        self.sampled > 0 && self.dormant * 2 > self.sampled
    }

    pub fn describe(&self) -> String {
        format!(
            "{} of {} sampled inputs had been unspent for over {DORMANT_AFTER_DAYS} days \
             (median {} days)",
            self.dormant, self.sampled, self.median_age_days
        )
    }
}

/// Ages of inputs, from the consolidation's block time and the block times of the
/// transactions that created the sampled inputs. `None` if nothing could be sampled.
pub fn dormancy(spend_time: u64, input_created: &[u64]) -> Option<Dormancy> {
    if input_created.is_empty() {
        return None;
    }
    let mut ages: Vec<u64> = input_created
        .iter()
        .map(|t| spend_time.saturating_sub(*t) / 86_400)
        .collect();
    ages.sort_unstable();
    Some(Dormancy {
        sampled: ages.len(),
        dormant: ages.iter().filter(|a| **a > DORMANT_AFTER_DAYS).count(),
        median_age_days: ages[ages.len() / 2],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    #[test]
    fn thief_consolidation_is_dormant() {
        // Bitfinex 2022: inputs created ~2,009 days earlier.
        let t = 2_000 * DAY + 10 * DAY;
        let d = dormancy(t, &[10 * DAY; 5]).unwrap();
        assert!(d.mostly_dormant());
        assert_eq!(d.median_age_days, 2_000);
    }

    #[test]
    fn deposit_sweeps_are_not() {
        // Silk Road 2012 sweep: 6–40 days; exchange sweeps: hours.
        let t = 100 * DAY;
        let d = dormancy(t, &[94 * DAY, 78 * DAY, 61 * DAY, 69 * DAY, 74 * DAY]).unwrap();
        assert!(!d.mostly_dormant());
        assert!(!dormancy(t, &[t - 3_600; 5]).unwrap().mostly_dormant());
    }

    #[test]
    fn needs_a_majority() {
        let t = 1_000 * DAY;
        assert!(!dormancy(t, &[0, 0, t, t]).unwrap().mostly_dormant());
        assert!(dormancy(t, &[0, 0, t]).unwrap().mostly_dormant());
        assert_eq!(dormancy(t, &[]), None);
    }
}
