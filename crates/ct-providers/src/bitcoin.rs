//! Bitcoin data access, independent of the source.

use async_trait::async_trait;
use ct_core::{Address, AddressSummary, Chain, OutputSpend, UtxoTx};

use crate::error::{ProviderError, Result};

/// Everything the Bitcoin tracer needs from a data source.
#[async_trait]
pub trait BitcoinProvider: Send + Sync {
    /// Short name for messages and provenance, e.g. "mempool.space".
    fn name(&self) -> &str;

    async fn tip_height(&self) -> Result<u64>;

    async fn address_summary(&self, address: &Address) -> Result<AddressSummary>;

    /// Confirmed transactions for an address, newest first, one page at a time.
    /// Pass the last txid of the previous page to get the next one. An empty page means
    /// there are no more.
    async fn address_txs(&self, address: &Address, after_txid: Option<&str>)
    -> Result<Vec<UtxoTx>>;

    async fn tx(&self, txid: &str) -> Result<UtxoTx>;

    /// Spend status of every output of `txid`, in output order.
    async fn outspends(&self, txid: &str) -> Result<Vec<OutputSpend>>;
}

/// Validate a txid (64 hex chars) and return it lowercased.
pub fn normalize_txid(txid: &str) -> Result<String> {
    let t = txid.trim();
    if t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(t.to_ascii_lowercase())
    } else {
        Err(ProviderError::InvalidInput(format!(
            "not a transaction id (expected 64 hex characters): {txid}"
        )))
    }
}

pub(crate) fn require_bitcoin(address: &Address) -> Result<()> {
    if address.chain() == Chain::Bitcoin {
        Ok(())
    } else {
        Err(ProviderError::InvalidInput(format!(
            "{address} is a {} address, not Bitcoin",
            address.chain()
        )))
    }
}

/// Tries each provider in order, moving on only for failures another source might not have
/// (rate limits, outages, bad responses). "Not found" and invalid input are returned as-is.
pub struct FallbackBitcoin {
    providers: Vec<Box<dyn BitcoinProvider>>,
}

impl FallbackBitcoin {
    pub fn new(providers: Vec<Box<dyn BitcoinProvider>>) -> Self {
        assert!(!providers.is_empty(), "at least one provider is required");
        FallbackBitcoin { providers }
    }
}

macro_rules! try_each {
    ($self:ident, |$p:ident| $call:expr) => {{
        let mut last_err = None;
        for $p in &$self.providers {
            match $call.await {
                Ok(v) => return Ok(v),
                Err(e) if e.should_fall_back() => {
                    tracing::warn!(provider = $p.name(), error = %e, "falling back");
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.expect("providers is non-empty"))
    }};
}

#[async_trait]
impl BitcoinProvider for FallbackBitcoin {
    fn name(&self) -> &str {
        self.providers[0].name()
    }

    async fn tip_height(&self) -> Result<u64> {
        try_each!(self, |p| p.tip_height())
    }

    async fn address_summary(&self, address: &Address) -> Result<AddressSummary> {
        try_each!(self, |p| p.address_summary(address))
    }

    async fn address_txs(
        &self,
        address: &Address,
        after_txid: Option<&str>,
    ) -> Result<Vec<UtxoTx>> {
        try_each!(self, |p| p.address_txs(address, after_txid))
    }

    async fn tx(&self, txid: &str) -> Result<UtxoTx> {
        try_each!(self, |p| p.tx(txid))
    }

    async fn outspends(&self, txid: &str) -> Result<Vec<OutputSpend>> {
        try_each!(self, |p| p.outspends(txid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txid_validation() {
        let t = "F4184FC596403B9D638783CF57ADFE4C75C605F6356FBC91338530E9831E9E16";
        assert_eq!(normalize_txid(t).unwrap(), t.to_ascii_lowercase());
        assert!(normalize_txid("abc").is_err());
        assert!(normalize_txid(&"g".repeat(64)).is_err());
    }
}
