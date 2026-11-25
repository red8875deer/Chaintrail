//! Exact token amounts in base units. Never use floating point for amounts.

use std::fmt;

use serde::{Deserialize, Serialize};

/// An amount in raw base units (satoshi, wei, sun, token units) with its decimals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Amount {
    pub raw: u128,
    pub decimals: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AmountError {
    #[error("not a non-negative integer: {0}")]
    NotAnInteger(String),
    /// Larger than any real balance on supported chains; typical of spam tokens.
    #[error("amount does not fit in 128 bits (likely a spam token): {0}")]
    Overflow(String),
}

impl Amount {
    pub const fn new(raw: u128, decimals: u8) -> Self {
        Amount { raw, decimals }
    }

    pub const fn zero(decimals: u8) -> Self {
        Amount { raw: 0, decimals }
    }

    /// Parse a decimal integer string of base units, as APIs return them.
    pub fn from_raw_str(s: &str, decimals: u8) -> Result<Self, AmountError> {
        let t = s.trim();
        if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
            return Err(AmountError::NotAnInteger(s.to_string()));
        }
        t.parse::<u128>()
            .map(|raw| Amount { raw, decimals })
            .map_err(|_| AmountError::Overflow(s.to_string()))
    }

    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    pub fn checked_add(self, other: Amount) -> Option<Amount> {
        debug_assert_eq!(
            self.decimals, other.decimals,
            "adding amounts of different assets"
        );
        self.raw
            .checked_add(other.raw)
            .map(|raw| Amount { raw, ..self })
    }

    pub fn saturating_sub(self, other: Amount) -> Amount {
        debug_assert_eq!(
            self.decimals, other.decimals,
            "subtracting amounts of different assets"
        );
        Amount {
            raw: self.raw.saturating_sub(other.raw),
            ..self
        }
    }

    /// Human-readable decimal string with trailing zeros removed, e.g. `"0.0015"`.
    pub fn to_decimal_string(&self) -> String {
        let d = self.decimals as u32;
        if d == 0 {
            return self.raw.to_string();
        }
        let digits = self.raw.to_string();
        let (int_part, frac_part) = if digits.len() as u32 > d {
            let split = digits.len() - d as usize;
            (digits[..split].to_string(), digits[split..].to_string())
        } else {
            (
                "0".to_string(),
                format!("{digits:0>width$}", width = d as usize),
            )
        };
        let frac = frac_part.trim_end_matches('0');
        if frac.is_empty() {
            int_part
        } else {
            format!("{int_part}.{frac}")
        }
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_decimal_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_decimals() {
        assert_eq!(Amount::new(5_000_000_000, 8).to_string(), "50");
        assert_eq!(Amount::new(150_000, 8).to_string(), "0.0015");
        assert_eq!(Amount::new(1, 18).to_string(), "0.000000000000000001");
        assert_eq!(Amount::new(1_234_567, 6).to_string(), "1.234567");
        assert_eq!(Amount::new(0, 6).to_string(), "0");
        assert_eq!(Amount::new(42, 0).to_string(), "42");
    }

    #[test]
    fn parses_raw_strings() {
        assert_eq!(
            Amount::from_raw_str("40000000", 6).unwrap().to_string(),
            "40"
        );
        assert!(matches!(
            Amount::from_raw_str("-1", 6),
            Err(AmountError::NotAnInteger(_))
        ));
        assert!(matches!(
            Amount::from_raw_str("1.5", 6),
            Err(AmountError::NotAnInteger(_))
        ));
        let huge = "1".repeat(60);
        assert!(matches!(
            Amount::from_raw_str(&huge, 18),
            Err(AmountError::Overflow(_))
        ));
    }

    #[test]
    fn arithmetic() {
        let a = Amount::new(10, 8);
        let b = Amount::new(3, 8);
        assert_eq!(a.checked_add(b).unwrap().raw, 13);
        assert_eq!(b.saturating_sub(a).raw, 0);
        assert!(
            Amount::new(u128::MAX, 0)
                .checked_add(Amount::new(1, 0))
                .is_none()
        );
    }
}
