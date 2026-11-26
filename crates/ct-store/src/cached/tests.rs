use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ct_core::{BlockRef, Chain, ScriptType, TxOutput};
use ct_providers::ProviderError;

use super::*;

const ADDR: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
const LONG: Duration = Duration::from_secs(3600);

/// Serves hand-built data and counts calls per method.
#[derive(Default)]
struct Fake {
    txs: HashMap<String, UtxoTx>,
    outspends: HashMap<String, Vec<OutputSpend>>,
    summaries: HashMap<String, AddressSummary>,
    /// Keyed by (address, after_txid or "").
    pages: HashMap<(String, String), Vec<UtxoTx>>,
    calls: Calls,
}

#[derive(Default)]
struct Calls {
    tip_height: AtomicUsize,
    address_summary: AtomicUsize,
    address_txs: AtomicUsize,
    tx: AtomicUsize,
    outspends: AtomicUsize,
}

fn count(c: &AtomicUsize) -> usize {
    c.load(Ordering::SeqCst)
}

fn bump(c: &AtomicUsize) {
    c.fetch_add(1, Ordering::SeqCst);
}

fn not_found(what: &str) -> ProviderError {
    ProviderError::NotFound(what.to_string())
}

#[async_trait]
impl BitcoinProvider for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    async fn tip_height(&self) -> Result<u64> {
        bump(&self.calls.tip_height);
        Ok(900_000)
    }

    async fn address_summary(&self, address: &Address) -> Result<AddressSummary> {
        bump(&self.calls.address_summary);
        self.summaries
            .get(address.as_str())
            .cloned()
            .ok_or_else(|| not_found(address.as_str()))
    }

    async fn address_txs(
        &self,
        address: &Address,
        after_txid: Option<&str>,
    ) -> Result<Vec<UtxoTx>> {
        bump(&self.calls.address_txs);
        let key = (
            address.as_str().to_string(),
            after_txid.unwrap_or("").to_string(),
        );
        self.pages
            .get(&key)
            .cloned()
            .ok_or_else(|| not_found(address.as_str()))
    }

    async fn tx(&self, txid: &str) -> Result<UtxoTx> {
        bump(&self.calls.tx);
        self.txs.get(txid).cloned().ok_or_else(|| not_found(txid))
    }

    async fn outspends(&self, txid: &str) -> Result<Vec<OutputSpend>> {
        bump(&self.calls.outspends);
        self.outspends
            .get(txid)
            .cloned()
            .ok_or_else(|| not_found(txid))
    }
}

impl Fake {
    fn with_tx(mut self, tx: &UtxoTx) -> Self {
        self.txs.insert(tx.txid.clone(), tx.clone());
        self
    }

    fn with_outspends(mut self, txid: &str, spends: Vec<OutputSpend>) -> Self {
        self.outspends.insert(txid.to_string(), spends);
        self
    }

    fn with_page(mut self, after: Option<&str>, txs: &[&UtxoTx]) -> Self {
        let key = (ADDR.to_string(), after.unwrap_or("").to_string());
        self.pages
            .insert(key, txs.iter().map(|&t| t.clone()).collect());
        self
    }
}

fn addr() -> Address {
    Address::parse(Chain::Bitcoin, ADDR).unwrap()
}

/// A valid lowercase txid with hex letters in it.
fn txid(n: u8) -> String {
    format!("{:064x}", 0xab00 + u32::from(n))
}

fn block() -> BlockRef {
    BlockRef {
        height: 800_000,
        hash: "00".repeat(32),
        time: 1_700_000_000,
    }
}

fn tx(n: u8, confirmed: bool, outputs: &[ScriptType]) -> UtxoTx {
    UtxoTx {
        chain: Chain::Bitcoin,
        txid: txid(n),
        block: confirmed.then(block),
        version: 2,
        locktime: 0,
        inputs: vec![],
        outputs: outputs
            .iter()
            .enumerate()
            .map(|(i, st)| TxOutput {
                index: i as u32,
                address: st.is_spendable().then(addr),
                value: if st.is_spendable() { 10_000 } else { 0 },
                script_type: st.clone(),
                script_pubkey: "00".into(),
            })
            .collect(),
        fee: 200,
        vsize: 141,
    }
}

fn spent(confirmed: bool) -> OutputSpend {
    OutputSpend {
        spent: true,
        spending_txid: Some(txid(200)),
        spending_vin: Some(0),
        block: confirmed.then(block),
    }
}

fn unspent() -> OutputSpend {
    OutputSpend {
        spent: false,
        spending_txid: None,
        spending_vin: None,
        block: None,
    }
}

fn summary() -> AddressSummary {
    AddressSummary {
        address: addr(),
        tx_count: 2,
        received_outputs: 2,
        received: 20_000,
        spent_outputs: 1,
        spent: 10_000,
        unconfirmed_tx_count: 0,
        unconfirmed_delta: 0,
    }
}

fn cached(fake: Fake, ttl: Duration) -> CachedBitcoin<Fake> {
    CachedBitcoin::new(
        fake,
        Store::open_in_memory().unwrap(),
        CachePolicy { volatile_ttl: ttl },
    )
}

fn stats(hits: u64, misses: u64) -> CacheStats {
    CacheStats { hits, misses }
}

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        TempDir(
            std::env::temp_dir().join(format!("ct-store-{name}-{}-{nanos}", std::process::id())),
        )
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn second_tx_call_is_a_hit() {
    let t = tx(1, true, &[ScriptType::P2wpkh]);
    let c = cached(Fake::default().with_tx(&t), LONG);

    assert_eq!(c.tx(&txid(1)).await.unwrap(), t);
    // Same txid in another case and with whitespace hits the same normalised key.
    let messy = format!(" {} ", txid(1).to_uppercase());
    assert_eq!(c.tx(&messy).await.unwrap(), t);

    assert_eq!(count(&c.inner.calls.tx), 1);
    assert_eq!(c.stats(), stats(1, 1));
    assert_eq!(c.name(), "fake");
}

#[tokio::test]
async fn unconfirmed_tx_not_cached() {
    let t = tx(1, false, &[ScriptType::P2wpkh]);
    let c = cached(Fake::default().with_tx(&t), LONG);

    assert_eq!(c.tx(&txid(1)).await.unwrap(), t);
    assert_eq!(c.tx(&txid(1)).await.unwrap(), t);

    assert_eq!(count(&c.inner.calls.tx), 2);
    assert_eq!(c.stats(), stats(0, 2));
}

#[tokio::test]
async fn invalid_txid_rejected_without_lookup() {
    let c = cached(Fake::default(), LONG);
    assert!(matches!(
        c.tx("abc").await,
        Err(ProviderError::InvalidInput(_))
    ));
    assert_eq!(count(&c.inner.calls.tx), 0);
    assert_eq!(c.stats(), CacheStats::default());
}

#[tokio::test]
async fn provider_errors_not_cached() {
    let c = cached(Fake::default(), LONG);
    assert!(matches!(
        c.tx(&txid(1)).await,
        Err(ProviderError::NotFound(_))
    ));
    assert!(c.tx(&txid(1)).await.is_err());
    assert_eq!(count(&c.inner.calls.tx), 2);
}

#[tokio::test]
async fn tip_height_never_cached() {
    let c = cached(Fake::default(), LONG);
    assert_eq!(c.tip_height().await.unwrap(), 900_000);
    assert_eq!(c.tip_height().await.unwrap(), 900_000);
    assert_eq!(count(&c.inner.calls.tip_height), 2);
    assert_eq!(c.stats(), CacheStats::default());
}

#[tokio::test]
async fn final_outspends_served_after_ttl() {
    let all_spent = vec![spent(true), spent(true)];
    let c = cached(
        Fake::default().with_outspends(&txid(1), all_spent.clone()),
        Duration::ZERO,
    );

    assert_eq!(c.outspends(&txid(1)).await.unwrap(), all_spent);
    assert_eq!(c.outspends(&txid(1)).await.unwrap(), all_spent);

    assert_eq!(count(&c.inner.calls.outspends), 1);
    assert_eq!(c.stats(), stats(1, 1));
}

#[tokio::test]
async fn non_final_outspends_expire() {
    // An unspent spendable output, with its tx cached.
    let t = tx(1, true, &[ScriptType::P2wpkh, ScriptType::P2wpkh]);
    // A spend that is not yet confirmed can still change.
    let t2 = tx(2, true, &[ScriptType::P2wpkh]);
    let c = cached(
        Fake::default()
            .with_tx(&t)
            .with_tx(&t2)
            .with_outspends(&txid(1), vec![spent(true), unspent()])
            .with_outspends(&txid(2), vec![spent(false)]),
        Duration::ZERO,
    );
    c.tx(&txid(1)).await.unwrap();
    c.tx(&txid(2)).await.unwrap();

    for _ in 0..2 {
        c.outspends(&txid(1)).await.unwrap();
        c.outspends(&txid(2)).await.unwrap();
    }
    assert_eq!(count(&c.inner.calls.outspends), 4);
}

#[tokio::test]
async fn non_final_outspends_served_within_ttl() {
    let c = cached(
        Fake::default().with_outspends(&txid(1), vec![unspent()]),
        LONG,
    );
    c.outspends(&txid(1)).await.unwrap();
    c.outspends(&txid(1)).await.unwrap();
    assert_eq!(count(&c.inner.calls.outspends), 1);
    assert_eq!(c.stats(), stats(1, 1));
}

#[tokio::test]
async fn unspent_op_return_is_final_once_tx_cached() {
    let t = tx(1, true, &[ScriptType::P2wpkh, ScriptType::OpReturn]);
    let spends = vec![spent(true), unspent()];
    let c = cached(
        Fake::default()
            .with_tx(&t)
            .with_outspends(&txid(1), spends.clone()),
        Duration::ZERO,
    );

    // Without the tx in the cache, finality cannot be shown: refetched every time.
    c.outspends(&txid(1)).await.unwrap();
    c.outspends(&txid(1)).await.unwrap();
    assert_eq!(count(&c.inner.calls.outspends), 2);

    // Once the tx is cached, the stored row is final.
    c.tx(&txid(1)).await.unwrap();
    assert_eq!(c.outspends(&txid(1)).await.unwrap(), spends);
    assert_eq!(count(&c.inner.calls.outspends), 2);
}

#[test]
fn outspends_finality_rules() {
    let op_ret = tx(1, true, &[ScriptType::P2wpkh, ScriptType::OpReturn]);
    let plain = tx(1, true, &[ScriptType::P2wpkh, ScriptType::P2wpkh]);

    assert!(outspends_final(&[spent(true), spent(true)], None));
    assert!(!outspends_final(&[spent(true), unspent()], None));
    assert!(outspends_final(&[spent(true), unspent()], Some(&op_ret)));
    assert!(!outspends_final(&[spent(true), unspent()], Some(&plain)));
    // The spendable output's spend is unconfirmed.
    assert!(!outspends_final(&[spent(false), unspent()], Some(&op_ret)));
    // Output count disagrees with the tx.
    assert!(!outspends_final(&[unspent()], Some(&op_ret)));
}

#[tokio::test]
async fn address_summary_expires() {
    let mut fake = Fake::default();
    fake.summaries.insert(ADDR.into(), summary());

    let c = cached(fake, Duration::ZERO);
    assert_eq!(c.address_summary(&addr()).await.unwrap(), summary());
    assert_eq!(c.address_summary(&addr()).await.unwrap(), summary());
    assert_eq!(count(&c.inner.calls.address_summary), 2);

    let mut fake = Fake::default();
    fake.summaries.insert(ADDR.into(), summary());
    let c = cached(fake, LONG);
    c.address_summary(&addr()).await.unwrap();
    assert_eq!(c.address_summary(&addr()).await.unwrap(), summary());
    assert_eq!(count(&c.inner.calls.address_summary), 1);
    assert_eq!(c.stats(), stats(1, 1));
}

#[tokio::test]
async fn address_pages() {
    let (t3, t2, t1) = (
        tx(3, true, &[ScriptType::P2wpkh]),
        tx(2, true, &[ScriptType::P2wpkh]),
        tx(1, true, &[ScriptType::P2wpkh]),
    );
    let c = cached(
        Fake::default()
            .with_page(None, &[&t3, &t2])
            .with_page(Some(&txid(2)), &[&t1]),
        Duration::ZERO,
    );

    // Newest page: volatile, so with TTL 0 it is refetched.
    assert_eq!(
        c.address_txs(&addr(), None).await.unwrap(),
        vec![t3.clone(), t2.clone()]
    );
    c.address_txs(&addr(), None).await.unwrap();
    assert_eq!(count(&c.inner.calls.address_txs), 2);

    // Older page: permanent, even with TTL 0. Order is preserved.
    let older = Some(txid(2));
    assert_eq!(
        c.address_txs(&addr(), older.as_deref()).await.unwrap(),
        vec![t1.clone()]
    );
    let upper = txid(2).to_uppercase();
    assert_eq!(
        c.address_txs(&addr(), Some(&upper)).await.unwrap(),
        vec![t1.clone()]
    );
    assert_eq!(count(&c.inner.calls.address_txs), 3);

    // The page's txs went into the tx table.
    assert_eq!(c.tx(&txid(3)).await.unwrap(), t3);
    assert_eq!(c.tx(&txid(1)).await.unwrap(), t1);
    assert_eq!(count(&c.inner.calls.tx), 0);
    assert_eq!(c.stats(), stats(3, 3));
}

#[tokio::test]
async fn newest_page_served_within_ttl() {
    let t1 = tx(1, true, &[ScriptType::P2wpkh]);
    let c = cached(Fake::default().with_page(None, &[&t1]), LONG);
    c.address_txs(&addr(), None).await.unwrap();
    assert_eq!(c.address_txs(&addr(), None).await.unwrap(), vec![t1]);
    assert_eq!(count(&c.inner.calls.address_txs), 1);
}

#[tokio::test]
async fn page_with_unconfirmed_tx_not_cached() {
    let (t2, t1) = (
        tx(2, false, &[ScriptType::P2wpkh]),
        tx(1, true, &[ScriptType::P2wpkh]),
    );
    let after = txid(9);
    let c = cached(Fake::default().with_page(Some(&after), &[&t2, &t1]), LONG);
    c.address_txs(&addr(), Some(&after)).await.unwrap();
    c.address_txs(&addr(), Some(&after)).await.unwrap();
    assert_eq!(count(&c.inner.calls.address_txs), 2);
}

#[tokio::test]
async fn page_with_missing_tx_is_a_miss() {
    let (t2, t1) = (
        tx(2, true, &[ScriptType::P2wpkh]),
        tx(1, true, &[ScriptType::P2wpkh]),
    );
    let after = txid(9);
    let c = cached(Fake::default().with_page(Some(&after), &[&t2, &t1]), LONG);
    c.address_txs(&addr(), Some(&after)).await.unwrap();
    c.store
        .execute_for_test(&format!("DELETE FROM btc_tx WHERE txid = '{}'", txid(1)));

    assert_eq!(
        c.address_txs(&addr(), Some(&after)).await.unwrap(),
        vec![t2, t1]
    );
    assert_eq!(count(&c.inner.calls.address_txs), 2);
    assert_eq!(c.stats(), stats(0, 2));
}

#[tokio::test]
async fn corrupt_row_is_a_miss_and_gets_replaced() {
    let t = tx(1, true, &[ScriptType::P2wpkh]);
    let c = cached(Fake::default().with_tx(&t), LONG);
    c.tx(&txid(1)).await.unwrap();
    c.store
        .execute_for_test("UPDATE btc_tx SET json = '{not json'");

    assert_eq!(c.tx(&txid(1)).await.unwrap(), t);
    assert_eq!(count(&c.inner.calls.tx), 2);
    // The refetch overwrote the bad row.
    assert_eq!(c.tx(&txid(1)).await.unwrap(), t);
    assert_eq!(count(&c.inner.calls.tx), 2);
    assert_eq!(c.stats(), stats(1, 2));
}

#[tokio::test]
async fn failed_cache_write_does_not_fail_request() {
    let t = tx(1, true, &[ScriptType::P2wpkh]);
    let c = cached(Fake::default().with_tx(&t), LONG);
    // Every write now fails.
    c.store.execute_for_test("PRAGMA query_only = ON");

    assert_eq!(c.tx(&txid(1)).await.unwrap(), t);
    assert_eq!(c.tx(&txid(1)).await.unwrap(), t);
    assert_eq!(count(&c.inner.calls.tx), 2);
}

#[tokio::test]
async fn cache_persists_across_open() {
    let dir = TempDir::new("persist");
    // Parent directories are created on open.
    let path = dir.0.join("nested").join("cache.sqlite");
    let t = tx(1, true, &[ScriptType::P2wpkh]);
    let spends = vec![spent(true)];

    {
        let c = CachedBitcoin::new(
            Fake::default()
                .with_tx(&t)
                .with_outspends(&txid(1), spends.clone()),
            Store::open(&path).unwrap(),
            CachePolicy::default(),
        );
        c.tx(&txid(1)).await.unwrap();
        c.outspends(&txid(1)).await.unwrap();
        assert_eq!(c.stats(), stats(0, 2));
    }

    // A provider with no data: everything must come from the file.
    let c = CachedBitcoin::new(
        Fake::default(),
        Store::open(&path).unwrap(),
        CachePolicy::default(),
    );
    assert_eq!(c.tx(&txid(1)).await.unwrap(), t);
    assert_eq!(c.outspends(&txid(1)).await.unwrap(), spends);
    assert_eq!(count(&c.inner.calls.tx), 0);
    assert_eq!(count(&c.inner.calls.outspends), 0);
    assert_eq!(c.stats(), stats(2, 0));
}
