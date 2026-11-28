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

    /// An address's activity counts for the busy-wallet check. Counts only grow, so a cached
    /// summary that is some days old is still a safe lower bound (caches keep it longer than
    /// a summary shown as the current balance).
    async fn address_activity(&self, address: &Address) -> Result<AddressSummary> {
        self.address_summary(address).await
    }

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
    /// Until when each provider is skipped after it failed (rate limited, down, blocked), so
    /// later requests go straight to the next one instead of paying its retries again.
    cooldown: Vec<std::sync::Mutex<Option<std::time::Instant>>>,
}

/// How long a failing provider is skipped.
pub const PROVIDER_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(60);

impl FallbackBitcoin {
    pub fn new(providers: Vec<Box<dyn BitcoinProvider>>) -> Self {
        assert!(!providers.is_empty(), "at least one provider is required");
        let cooldown = providers
            .iter()
            .map(|_| std::sync::Mutex::new(None))
            .collect();
        FallbackBitcoin {
            providers,
            cooldown,
        }
    }

    /// Providers to try, in order: those not cooling down; if all are, all of them.
    fn order(&self) -> Vec<usize> {
        let now = std::time::Instant::now();
        let ready: Vec<usize> = (0..self.providers.len())
            .filter(|&i| {
                self.cooldown[i]
                    .lock()
                    .map(|c| c.is_none_or(|t| t <= now))
                    .unwrap_or(true)
            })
            .collect();
        if ready.is_empty() {
            (0..self.providers.len()).collect()
        } else {
            ready
        }
    }

    fn failed(&self, i: usize) {
        if let Ok(mut c) = self.cooldown[i].lock() {
            *c = Some(std::time::Instant::now() + PROVIDER_COOLDOWN);
        }
    }
}

macro_rules! try_each {
    ($self:ident, |$p:ident| $call:expr) => {{
        let mut last_err = None;
        for i in $self.order() {
            let $p = &$self.providers[i];
            match $call.await {
                Ok(v) => return Ok(v),
                Err(e) if e.should_fall_back() => {
                    tracing::warn!(provider = $p.name(), error = %e, "falling back");
                    $self.failed(i);
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

/// A provider that fails every request with [`ProviderError::Cancelled`] once `stop` is set:
/// how a running trace is stopped (it then reports what it found so far).
pub struct Stoppable<'a> {
    inner: &'a dyn BitcoinProvider,
    stop: &'a std::sync::atomic::AtomicBool,
}

impl<'a> Stoppable<'a> {
    pub fn new(inner: &'a dyn BitcoinProvider, stop: &'a std::sync::atomic::AtomicBool) -> Self {
        Stoppable { inner, stop }
    }

    fn check(&self) -> Result<()> {
        if self.stop.load(std::sync::atomic::Ordering::Relaxed) {
            Err(ProviderError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl BitcoinProvider for Stoppable<'_> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn tip_height(&self) -> Result<u64> {
        self.check()?;
        self.inner.tip_height().await
    }

    async fn address_summary(&self, address: &Address) -> Result<AddressSummary> {
        self.check()?;
        self.inner.address_summary(address).await
    }

    async fn address_activity(&self, address: &Address) -> Result<AddressSummary> {
        self.check()?;
        self.inner.address_activity(address).await
    }

    async fn address_txs(
        &self,
        address: &Address,
        after_txid: Option<&str>,
    ) -> Result<Vec<UtxoTx>> {
        self.check()?;
        self.inner.address_txs(address, after_txid).await
    }

    async fn tx(&self, txid: &str) -> Result<UtxoTx> {
        self.check()?;
        self.inner.tx(txid).await
    }

    async fn outspends(&self, txid: &str) -> Result<Vec<OutputSpend>> {
        self.check()?;
        self.inner.outspends(txid).await
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
