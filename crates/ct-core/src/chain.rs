use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A supported blockchain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    Bitcoin,
    Ethereum,
    Tron,
}

impl Chain {
    pub const ALL: [Chain; 3] = [Chain::Bitcoin, Chain::Ethereum, Chain::Tron];

    /// Symbol of the chain's native asset.
    pub fn native_symbol(self) -> &'static str {
        match self {
            Chain::Bitcoin => "BTC",
            Chain::Ethereum => "ETH",
            Chain::Tron => "TRX",
        }
    }

    /// Decimals of the chain's native asset (sats, wei, sun).
    pub fn native_decimals(self) -> u8 {
        match self {
            Chain::Bitcoin => 8,
            Chain::Ethereum => 18,
            Chain::Tron => 6,
        }
    }

    /// Whether the chain uses the UTXO model (as opposed to accounts).
    pub fn is_utxo(self) -> bool {
        matches!(self, Chain::Bitcoin)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Chain::Bitcoin => "bitcoin",
            Chain::Ethereum => "ethereum",
            Chain::Tron => "tron",
        }
    }
}

impl fmt::Display for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown chain: {0}")]
pub struct UnknownChain(pub String);

impl FromStr for Chain {
    type Err = UnknownChain;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bitcoin" | "btc" => Ok(Chain::Bitcoin),
            "ethereum" | "eth" => Ok(Chain::Ethereum),
            "tron" | "trx" => Ok(Chain::Tron),
            _ => Err(UnknownChain(s.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display_round_trip() {
        for chain in Chain::ALL {
            assert_eq!(chain.as_str().parse::<Chain>().unwrap(), chain);
        }
        assert_eq!("BTC".parse::<Chain>().unwrap(), Chain::Bitcoin);
        assert!("solana".parse::<Chain>().is_err());
    }
}
