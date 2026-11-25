//! Validated, normalised blockchain addresses.
//!
//! Validation includes checksums (Base58Check, Bech32/Bech32m, EIP-55 when mixed case),
//! so a mistyped address is rejected instead of silently producing an empty trace.
//! Only mainnet addresses are accepted.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

use crate::chain::Chain;

/// Bitcoin mainnet Base58 version bytes.
const BTC_P2PKH_VERSION: u8 = 0x00;
const BTC_P2SH_VERSION: u8 = 0x05;
/// Tron address prefix byte (base58 addresses start with `T`).
const TRON_PREFIX: u8 = 0x41;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    #[error("empty address")]
    Empty,
    #[error("not a valid {chain} address: {reason}")]
    Invalid { chain: Chain, reason: String },
}

fn invalid(chain: Chain, reason: impl Into<String>) -> AddressError {
    AddressError::Invalid {
        chain,
        reason: reason.into(),
    }
}

/// The encoding of a Bitcoin address, which tells us its script type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitcoinAddressKind {
    P2pkh,
    P2sh,
    /// SegWit v0, 20-byte program.
    P2wpkh,
    /// SegWit v0, 32-byte program.
    P2wsh,
    /// SegWit v1, 32-byte program.
    P2tr,
    /// Other SegWit versions/lengths (valid but non-standard today).
    OtherSegwit,
}

/// A validated address on a specific chain.
///
/// Stored in a canonical form so equal addresses compare equal:
/// Bitcoin bech32 lowercase, Bitcoin base58 as-is, Ethereum lowercase hex with `0x`,
/// Tron base58.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "AddressRepr", into = "AddressRepr")]
pub struct Address {
    chain: Chain,
    value: String,
}

#[derive(Serialize, Deserialize)]
struct AddressRepr {
    chain: Chain,
    value: String,
}

impl TryFrom<AddressRepr> for Address {
    type Error = AddressError;
    fn try_from(r: AddressRepr) -> Result<Self, Self::Error> {
        Address::parse(r.chain, &r.value)
    }
}

impl From<Address> for AddressRepr {
    fn from(a: Address) -> Self {
        AddressRepr {
            chain: a.chain,
            value: a.value,
        }
    }
}

impl Address {
    /// Parse and validate an address for `chain`.
    pub fn parse(chain: Chain, input: &str) -> Result<Self, AddressError> {
        let s = input.trim();
        if s.is_empty() {
            return Err(AddressError::Empty);
        }
        let value = match chain {
            Chain::Bitcoin => normalize_bitcoin(s)?,
            Chain::Ethereum => normalize_ethereum(s)?,
            Chain::Tron => normalize_tron(s)?,
        };
        Ok(Address { chain, value })
    }

    /// Detect which supported chain an address belongs to.
    ///
    /// Formats do not overlap (Bitcoin `1`/`3`/`bc1`, Ethereum `0x`, Tron `T`), so at most one
    /// chain matches.
    pub fn detect(input: &str) -> Result<Self, AddressError> {
        let s = input.trim();
        if s.is_empty() {
            return Err(AddressError::Empty);
        }
        let chain = if s.starts_with("0x") || s.starts_with("0X") {
            Chain::Ethereum
        } else if s.starts_with('T') {
            Chain::Tron
        } else {
            Chain::Bitcoin
        };
        Address::parse(chain, s)
    }

    /// Build a Tron address from the 20-byte EVM-style hex form that TRC-20 event logs use
    /// (`0x` + 40 hex chars, or `41` + 40 hex chars).
    pub fn tron_from_hex(hex_str: &str) -> Result<Self, AddressError> {
        let h = hex_str.trim();
        let h = h
            .strip_prefix("0x")
            .or_else(|| h.strip_prefix("0X"))
            .unwrap_or(h);
        let body = match h.len() {
            40 => h,
            42 if h[..2].eq_ignore_ascii_case("41") => &h[2..],
            _ => return Err(invalid(Chain::Tron, "hex form must be 20 bytes")),
        };
        let bytes = hex::decode(body).map_err(|_| invalid(Chain::Tron, "invalid hex"))?;
        let mut payload = Vec::with_capacity(21);
        payload.push(TRON_PREFIX);
        payload.extend_from_slice(&bytes);
        Ok(Address {
            chain: Chain::Tron,
            value: bs58::encode(payload).with_check().into_string(),
        })
    }

    pub fn chain(&self) -> Chain {
        self.chain
    }

    /// Canonical string form.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// For Bitcoin addresses, the address encoding (which implies the script type).
    pub fn bitcoin_kind(&self) -> Option<BitcoinAddressKind> {
        if self.chain != Chain::Bitcoin {
            return None;
        }
        bitcoin_kind(&self.value)
    }

    /// Ethereum address with EIP-55 mixed-case checksum, for display.
    pub fn eip55(&self) -> Option<String> {
        (self.chain == Chain::Ethereum).then(|| eip55_checksum(&self.value[2..]))
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

fn base58_error(chain: Chain, e: bs58::decode::Error) -> AddressError {
    match e {
        bs58::decode::Error::InvalidChecksum { .. } => {
            invalid(chain, "checksum does not match; check for a typo")
        }
        bs58::decode::Error::InvalidCharacter { character, .. } => {
            invalid(chain, format!("invalid character '{character}'"))
        }
        other => invalid(chain, other.to_string()),
    }
}

fn normalize_bitcoin(s: &str) -> Result<String, AddressError> {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("bc1") {
        let (hrp, _version, _program) = bech32::segwit::decode(s)
            .map_err(|e| invalid(Chain::Bitcoin, format!("bech32: {e}")))?;
        if hrp != bech32::hrp::BC {
            return Err(invalid(Chain::Bitcoin, "not a mainnet address"));
        }
        return Ok(lower);
    }
    let payload = bs58::decode(s)
        .with_check(None)
        .into_vec()
        .map_err(|e| base58_error(Chain::Bitcoin, e))?;
    if payload.len() != 21 {
        return Err(invalid(Chain::Bitcoin, "wrong payload length"));
    }
    match payload[0] {
        BTC_P2PKH_VERSION | BTC_P2SH_VERSION => Ok(s.to_string()),
        v => Err(invalid(
            Chain::Bitcoin,
            format!("unsupported version byte 0x{v:02x} (not mainnet P2PKH/P2SH)"),
        )),
    }
}

fn bitcoin_kind(value: &str) -> Option<BitcoinAddressKind> {
    if value.starts_with("bc1") {
        let (_, version, program) = bech32::segwit::decode(value).ok()?;
        return Some(match (version.to_u8(), program.len()) {
            (0, 20) => BitcoinAddressKind::P2wpkh,
            (0, 32) => BitcoinAddressKind::P2wsh,
            (1, 32) => BitcoinAddressKind::P2tr,
            _ => BitcoinAddressKind::OtherSegwit,
        });
    }
    match value.as_bytes().first()? {
        b'1' => Some(BitcoinAddressKind::P2pkh),
        b'3' => Some(BitcoinAddressKind::P2sh),
        _ => None,
    }
}

fn normalize_ethereum(s: &str) -> Result<String, AddressError> {
    let body = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .ok_or_else(|| invalid(Chain::Ethereum, "must start with 0x"))?;
    if body.len() != 40 || !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid(Chain::Ethereum, "must be 40 hex characters"));
    }
    let has_upper = body.bytes().any(|b| b.is_ascii_uppercase());
    let has_lower = body.bytes().any(|b| b.is_ascii_lowercase());
    if has_upper && has_lower && eip55_checksum(body)[2..] != *body {
        return Err(invalid(
            Chain::Ethereum,
            "mixed-case checksum (EIP-55) does not match; check for a typo",
        ));
    }
    Ok(format!("0x{}", body.to_ascii_lowercase()))
}

/// EIP-55 checksum encoding of a 40-char hex body (any case), returned with `0x`.
fn eip55_checksum(body: &str) -> String {
    let lower = body.to_ascii_lowercase();
    let hash = Keccak256::digest(lower.as_bytes());
    let mut out = String::with_capacity(42);
    out.push_str("0x");
    for (i, c) in lower.chars().enumerate() {
        let nibble = (hash[i / 2] >> (if i % 2 == 0 { 4 } else { 0 })) & 0x0f;
        if c.is_ascii_alphabetic() && nibble >= 8 {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn normalize_tron(s: &str) -> Result<String, AddressError> {
    let payload = bs58::decode(s)
        .with_check(None)
        .into_vec()
        .map_err(|e| base58_error(Chain::Tron, e))?;
    if payload.len() != 21 || payload[0] != TRON_PREFIX {
        return Err(invalid(Chain::Tron, "not a Tron mainnet address"));
    }
    Ok(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitcoin_legacy_and_p2sh() {
        let a = Address::parse(Chain::Bitcoin, "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa").unwrap();
        assert_eq!(a.bitcoin_kind(), Some(BitcoinAddressKind::P2pkh));
        let p2sh = Address::parse(Chain::Bitcoin, "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy").unwrap();
        assert_eq!(p2sh.bitcoin_kind(), Some(BitcoinAddressKind::P2sh));
    }

    #[test]
    fn bitcoin_bad_checksum_rejected() {
        // Last character changed.
        assert!(Address::parse(Chain::Bitcoin, "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb").is_err());
    }

    #[test]
    fn bitcoin_bech32_and_taproot() {
        // BIP-173 / BIP-350 mainnet test vectors.
        let w =
            Address::parse(Chain::Bitcoin, "BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4").unwrap();
        assert_eq!(w.as_str(), "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4");
        assert_eq!(w.bitcoin_kind(), Some(BitcoinAddressKind::P2wpkh));
        let tr = Address::parse(
            Chain::Bitcoin,
            "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0",
        )
        .unwrap();
        assert_eq!(tr.bitcoin_kind(), Some(BitcoinAddressKind::P2tr));
    }

    #[test]
    fn bitcoin_testnet_rejected() {
        assert!(
            Address::parse(Chain::Bitcoin, "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx").is_err()
        );
        assert!(Address::parse(Chain::Bitcoin, "mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn").is_err());
    }

    #[test]
    fn ethereum_normalised_and_checksum_checked() {
        let a = Address::parse(
            Chain::Ethereum,
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        )
        .unwrap();
        assert_eq!(a.as_str(), "0xd8da6bf26964af9d7eed9e03e53415d37aa96045");
        assert_eq!(
            a.eip55().unwrap(),
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
        );
        // All-lowercase has no checksum and is accepted.
        assert!(
            Address::parse(
                Chain::Ethereum,
                "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"
            )
            .is_ok()
        );
        // Wrong case on one letter breaks the checksum.
        assert!(
            Address::parse(
                Chain::Ethereum,
                "0xD8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
            )
            .is_err()
        );
        assert!(Address::parse(Chain::Ethereum, "0x1234").is_err());
    }

    #[test]
    fn tron_base58_and_hex() {
        let usdt = Address::parse(Chain::Tron, "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t").unwrap();
        assert_eq!(usdt.chain(), Chain::Tron);
        assert!(Address::parse(Chain::Tron, "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6u").is_err());
        // The USDT contract's hex form.
        let from_hex =
            Address::tron_from_hex("0xa614f803b6fd780986a42c78ec9c7f77e6ded13c").unwrap();
        assert_eq!(from_hex, usdt);
        let from_41 = Address::tron_from_hex("41a614f803b6fd780986a42c78ec9c7f77e6ded13c").unwrap();
        assert_eq!(from_41, usdt);
    }

    #[test]
    fn detect_chain() {
        assert_eq!(
            Address::detect("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa")
                .unwrap()
                .chain(),
            Chain::Bitcoin
        );
        assert_eq!(
            Address::detect("0xdac17f958d2ee523a2206206994597c13d831ec7")
                .unwrap()
                .chain(),
            Chain::Ethereum
        );
        assert_eq!(
            Address::detect("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t")
                .unwrap()
                .chain(),
            Chain::Tron
        );
        assert_eq!(Address::detect("  "), Err(AddressError::Empty));
    }

    #[test]
    fn serde_round_trip_validates() {
        let a = Address::parse(Chain::Tron, "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t").unwrap();
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Address>(&json).unwrap(), a);
        let bad = r#"{"chain":"tron","value":"Tnotreal"}"#;
        assert!(serde_json::from_str::<Address>(bad).is_err());
    }
}
