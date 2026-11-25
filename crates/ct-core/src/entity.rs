//! Real-world entities and the labels that tie addresses to them.

use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::claim::Claim;

/// What kind of service or actor an address belongs to. Determines whether tracing stops
/// there (see `docs/TRACING.md` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityCategory {
    Exchange,
    Mixer,
    Bridge,
    Dex,
    Defi,
    SwapService,
    PaymentProcessor,
    WalletService,
    MiningPool,
    Gambling,
    Darknet,
    Scam,
    Ransomware,
    Hack,
    Sanctioned,
    Individual,
    /// Behaves like a service but we do not know which one.
    UnknownService,
}

impl EntityCategory {
    /// Whether funds reaching this category are a lead (and tracing stops there by default).
    /// Custodial services and protocols are where an investigator acts next: request records,
    /// ask for a freeze, or continue on another chain.
    pub fn is_endpoint(self) -> bool {
        !matches!(self, EntityCategory::Individual)
    }

    /// Whether funds can be followed through this kind of entity on-chain
    /// (value continues in another asset or on another chain).
    pub fn is_pass_through(self) -> bool {
        matches!(self, EntityCategory::Bridge | EntityCategory::Dex)
    }
}

/// A statement that an address belongs to an entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub address: Address,
    /// Entity name, e.g. "Binance". Empty when only the category is known.
    pub entity: String,
    pub category: EntityCategory,
    /// Always an `Attribution` (from a source) or an `Inference` (from behaviour).
    pub claim: Claim,
    /// Free-text note, e.g. "hot wallet 3".
    pub note: Option<String>,
}
