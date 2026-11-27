//! Behavioural service detection from address activity (`docs/TRACING.md` §2).
//!
//! Services (exchange hot wallets, payment processors, pools, gambling sites) both receive
//! and spend constantly. Addresses that only receive a lot (donation or vanity addresses,
//! dust targets like the genesis address) are not treated as services.

use ct_core::{AddressSummary, Confidence, EntityCategory};
use serde::{Deserialize, Serialize};

/// Transactions and spends for a Medium "likely service" hint.
pub const SERVICE_MEDIUM_TXS: u64 = 1_000;
pub const SERVICE_MEDIUM_SPENDS: u64 = 100;
/// Transactions and spends for a High hint.
pub const SERVICE_HIGH_TXS: u64 = 10_000;
pub const SERVICE_HIGH_SPENDS: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceHint {
    pub category: EntityCategory,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
}

pub fn service_from_summary(s: &AddressSummary) -> Option<ServiceHint> {
    let confidence = if s.tx_count >= SERVICE_HIGH_TXS && s.spent_outputs >= SERVICE_HIGH_SPENDS {
        Confidence::High
    } else if s.tx_count >= SERVICE_MEDIUM_TXS && s.spent_outputs >= SERVICE_MEDIUM_SPENDS {
        Confidence::Medium
    } else {
        return None;
    };
    Some(ServiceHint {
        category: EntityCategory::UnknownService,
        confidence,
        evidence: vec![
            format!(
                "address has {} transactions and has spent {} outputs (busy wallet)",
                s.tx_count, s.spent_outputs
            ),
            "counted over the address's whole history to date, not only up to this transfer".into(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_core::{Address, Chain};

    fn summary(tx_count: u64, spent_outputs: u64) -> AddressSummary {
        AddressSummary {
            address: Address::parse(Chain::Bitcoin, "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa").unwrap(),
            tx_count,
            received_outputs: tx_count,
            received: 0,
            spent_outputs,
            spent: 0,
            unconfirmed_tx_count: 0,
            unconfirmed_delta: 0,
        }
    }

    #[test]
    fn busy_wallets_are_services() {
        assert_eq!(
            service_from_summary(&summary(25_000, 9_000))
                .unwrap()
                .confidence,
            Confidence::High
        );
        assert_eq!(
            service_from_summary(&summary(2_000, 500))
                .unwrap()
                .confidence,
            Confidence::Medium
        );
    }

    #[test]
    fn receive_only_and_small_wallets_are_not() {
        // Genesis address: ~67k incoming dust txs, never spent.
        assert!(service_from_summary(&summary(67_598, 0)).is_none());
        assert!(service_from_summary(&summary(40, 20)).is_none());
    }
}
