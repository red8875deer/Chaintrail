//! [`CachedBitcoin`]: a caching wrapper around any [`BitcoinProvider`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use ct_core::{Address, AddressSummary, OutputSpend, UtxoTx};
use ct_providers::bitcoin::normalize_txid;
use ct_providers::{BitcoinProvider, Result};

use crate::error::StoreError;
use crate::store::Store;

/// How long volatile data may be served from the cache.
#[derive(Debug, Clone)]
pub struct CachePolicy {
    /// Lifetime of data that can still change: address summaries, the newest page of an
    /// address's history, and spend status that is not yet final. Default: 10 minutes.
    /// `Duration::ZERO` disables caching of volatile data (permanent data is still cached).
    pub volatile_ttl: Duration,
}

impl Default for CachePolicy {
    fn default() -> Self {
        CachePolicy {
            volatile_ttl: Duration::from_secs(10 * 60),
        }
    }
}

/// Cache lookups since the wrapper was created. `tip_height` is never cached or counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
}

/// A [`BitcoinProvider`] that answers from the local [`Store`] when it can and otherwise
/// asks `inner`, saving what it fetched.
///
/// Confirmed blockchain data never changes, so it is kept forever. Anything that can still
/// change expires after [`CachePolicy::volatile_ttl`]. Per method:
///
/// - **`tx`**: served from the cache if present. A fetched tx is cached (forever) only if it
///   is confirmed (`block.is_some()`); unconfirmed txs are never cached.
/// - **`outspends`**: always cached, with its fetch time. The row is *final* (served forever)
///   if every output is spent by a confirmed tx, or if every output that is not spent by a
///   confirmed tx is unspent and unspendable (OP_RETURN) according to the cached tx with the
///   same txid. If that tx is not cached, the row is not final. Rows that are not final
///   expire after `volatile_ttl`.
/// - **`address_summary`**: cached for `volatile_ttl`.
/// - **`address_txs`**: the newest page (`after_txid = None`) is cached for `volatile_ttl`;
///   older pages (`Some(after_txid)`) cover settled history and are cached forever. A page
///   is stored as its list of txids, with each tx in the tx table. It is only cached when
///   every tx on it is confirmed, and it counts as a miss if any of its txs is missing from
///   the tx table.
/// - **`tip_height`**: never cached.
///
/// Txids are normalised with [`normalize_txid`] before being used as keys; addresses use
/// [`Address::as_str`].
///
/// The cache never makes a request fail: a database error or an undecodable row is logged
/// with `tracing::warn!` and treated as a miss, and a failed write is logged and the fetched
/// data returned anyway. Errors from `inner` are returned unchanged and never cached.
///
/// Chain reorganisations are not handled: a tx cached with one confirmation keeps its
/// block reference even if that block is later orphaned.
pub struct CachedBitcoin<P> {
    inner: P,
    store: Store,
    policy: CachePolicy,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl<P: BitcoinProvider> CachedBitcoin<P> {
    pub fn new(inner: P, store: Store, policy: CachePolicy) -> Self {
        CachedBitcoin {
            inner,
            store,
            policy,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
        }
    }

    /// Run a cache read and count it as a hit or miss. Read errors count as misses.
    async fn lookup<T, F>(&self, what: &'static str, read: F) -> Option<T>
    where
        F: FnOnce(&Store) -> Result<Option<T>, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let found = self.store.run(read).await.unwrap_or_else(|e| {
            tracing::warn!(what, error = %e, "cache read failed; fetching instead");
            None
        });
        let counter = if found.is_some() {
            &self.hits
        } else {
            &self.misses
        };
        counter.fetch_add(1, Ordering::Relaxed);
        found
    }

    /// Run a cache write. Failures are logged, never returned.
    async fn save<F>(&self, what: &'static str, write: F)
    where
        F: FnOnce(&Store) -> Result<(), StoreError> + Send + 'static,
    {
        if let Err(e) = self.store.run(write).await {
            tracing::warn!(what, error = %e, "cache write failed; result not cached");
        }
    }
}

fn spent_and_confirmed(s: &OutputSpend) -> bool {
    s.spent && s.block.is_some()
}

/// Whether an outspends list can never change (see [`CachedBitcoin`]).
fn outspends_final(spends: &[OutputSpend], tx: Option<&UtxoTx>) -> bool {
    if spends.iter().all(spent_and_confirmed) {
        return true;
    }
    let Some(tx) = tx else {
        return false;
    };
    tx.outputs.len() == spends.len()
        && spends
            .iter()
            .zip(&tx.outputs)
            .all(|(s, out)| spent_and_confirmed(s) || (!s.spent && !out.script_type.is_spendable()))
}

/// The cache key and value for a fetched tx, if it may be cached (confirmed, valid txid).
fn cacheable(tx: &UtxoTx) -> Option<(String, UtxoTx)> {
    tx.block.as_ref()?;
    let txid = normalize_txid(&tx.txid).ok()?;
    Some((txid, tx.clone()))
}

#[async_trait]
impl<P: BitcoinProvider> BitcoinProvider for CachedBitcoin<P> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn tip_height(&self) -> Result<u64> {
        self.inner.tip_height().await
    }

    async fn address_summary(&self, address: &Address) -> Result<AddressSummary> {
        let key = address.as_str().to_owned();
        let ttl = self.policy.volatile_ttl;
        let k = key.clone();
        let cached = self
            .lookup("address_summary", move |store| {
                let row = store.get_address_summary(&k)?;
                Ok(row.filter(|r| r.is_fresh(ttl)).map(|r| r.value))
            })
            .await;
        if let Some(summary) = cached {
            return Ok(summary);
        }

        let summary = self.inner.address_summary(address).await?;
        let s = summary.clone();
        self.save("address_summary", move |store| {
            store.put_address_summary(&key, &s)
        })
        .await;
        Ok(summary)
    }

    async fn address_txs(
        &self,
        address: &Address,
        after_txid: Option<&str>,
    ) -> Result<Vec<UtxoTx>> {
        let after = after_txid.map(normalize_txid).transpose()?;
        // Older pages are settled history; only the newest page can change.
        let ttl = after.is_none().then_some(self.policy.volatile_ttl);
        let key = address.as_str().to_owned();
        let after_key = after.clone().unwrap_or_default();

        let (k, a) = (key.clone(), after_key.clone());
        let cached = self
            .lookup("address_txs", move |store| {
                let Some(page) = store.get_address_page(&k, &a)? else {
                    return Ok(None);
                };
                if ttl.is_some_and(|ttl| !page.is_fresh(ttl)) {
                    return Ok(None);
                }
                let mut txs = Vec::with_capacity(page.value.len());
                for txid in &page.value {
                    match store.get_tx(txid)? {
                        Some(tx) => txs.push(tx),
                        None => return Ok(None),
                    }
                }
                Ok(Some(txs))
            })
            .await;
        if let Some(txs) = cached {
            return Ok(txs);
        }

        let txs = self.inner.address_txs(address, after.as_deref()).await?;
        match txs.iter().map(cacheable).collect::<Option<Vec<_>>>() {
            Some(entries) => {
                self.save("address_txs", move |store| {
                    store.put_address_page(&key, &after_key, &entries)
                })
                .await;
            }
            None => tracing::debug!(
                address = %address,
                "address page has an unconfirmed or malformed tx; not cached"
            ),
        }
        Ok(txs)
    }

    async fn tx(&self, txid: &str) -> Result<UtxoTx> {
        let txid = normalize_txid(txid)?;
        let k = txid.clone();
        if let Some(tx) = self.lookup("tx", move |store| store.get_tx(&k)).await {
            return Ok(tx);
        }

        let tx = self.inner.tx(&txid).await?;
        if let Some((key, t)) = cacheable(&tx) {
            self.save("tx", move |store| store.put_tx(&key, &t)).await;
        }
        Ok(tx)
    }

    async fn outspends(&self, txid: &str) -> Result<Vec<OutputSpend>> {
        let txid = normalize_txid(txid)?;
        let ttl = self.policy.volatile_ttl;
        let k = txid.clone();
        let cached = self
            .lookup("outspends", move |store| {
                let Some(row) = store.get_outspends(&k)? else {
                    return Ok(None);
                };
                if row.is_fresh(ttl) || row.value.iter().all(spent_and_confirmed) {
                    return Ok(Some(row.value));
                }
                let tx = store.get_tx(&k)?;
                Ok(outspends_final(&row.value, tx.as_ref()).then_some(row.value))
            })
            .await;
        if let Some(spends) = cached {
            return Ok(spends);
        }

        let spends = self.inner.outspends(&txid).await?;
        let s = spends.clone();
        self.save("outspends", move |store| store.put_outspends(&txid, &s))
            .await;
        Ok(spends)
    }
}

#[cfg(test)]
mod tests;
