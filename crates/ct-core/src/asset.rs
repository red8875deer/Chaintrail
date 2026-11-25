//! Assets: a chain's native coin or a token contract on it.

use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::chain::Chain;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Asset {
    pub chain: Chain,
    pub symbol: String,
    pub decimals: u8,
    /// Token contract; `None` for the native coin.
    pub contract: Option<Address>,
}

impl Asset {
    pub fn native(chain: Chain) -> Asset {
        Asset {
            chain,
            symbol: chain.native_symbol().to_string(),
            decimals: chain.native_decimals(),
            contract: None,
        }
    }

    pub fn token(chain: Chain, contract: Address, symbol: &str, decimals: u8) -> Asset {
        Asset {
            chain,
            symbol: symbol.to_string(),
            decimals,
            contract: Some(contract),
        }
    }

    pub fn is_native(&self) -> bool {
        self.contract.is_none()
    }

    /// Tokens the project explicitly targets (see ADR-0003). Contract addresses are the
    /// issuers' official deployments.
    pub fn well_known_tokens() -> Vec<Asset> {
        let t = |chain, addr: &str, sym, dec| {
            Asset::token(
                chain,
                Address::parse(chain, addr).expect("hard-coded address is valid"),
                sym,
                dec,
            )
        };
        vec![
            t(
                Chain::Ethereum,
                "0xdac17f958d2ee523a2206206994597c13d831ec7",
                "USDT",
                6,
            ),
            t(
                Chain::Ethereum,
                "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                "USDC",
                6,
            ),
            t(Chain::Tron, "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t", "USDT", 6),
        ]
    }

    /// Look up a well-known token by contract.
    pub fn well_known(contract: &Address) -> Option<Asset> {
        Self::well_known_tokens()
            .into_iter()
            .find(|a| a.contract.as_ref() == Some(contract))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natives() {
        let btc = Asset::native(Chain::Bitcoin);
        assert_eq!((btc.symbol.as_str(), btc.decimals), ("BTC", 8));
        assert!(btc.is_native());
        assert_eq!(Asset::native(Chain::Ethereum).decimals, 18);
        assert_eq!(Asset::native(Chain::Tron).decimals, 6);
    }

    #[test]
    fn well_known_lookup() {
        let usdt = Address::parse(Chain::Tron, "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t").unwrap();
        assert_eq!(Asset::well_known(&usdt).unwrap().symbol, "USDT");
        let usdc = Address::parse(
            Chain::Ethereum,
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        )
        .unwrap();
        assert_eq!(Asset::well_known(&usdc).unwrap().symbol, "USDC");
    }
}
