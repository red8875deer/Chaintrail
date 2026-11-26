//! The application service: everything the desktop app and `chaintrail serve` can ask for.
//!
//! One method per UI command, each taking a request struct and returning a view struct
//! (`views.rs`). Both front ends call exactly these methods, so they behave identically.

pub mod error;
pub mod labels;
pub mod views;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use ct_core::time::format_utc;
use ct_core::{Address, Chain, ScriptType};
use ct_heuristics::btc::{TxShape, classify, detect_change, is_peel_step};
use ct_labels::{LabelIndex, LabelRecord};
use ct_providers::BitcoinProvider;
use ct_providers::bitcoin::normalize_txid;
use ct_trace::{Progress, Seed, TraceConfig};

pub use error::{ErrorKind, Result, ServiceError};
use views::*;

/// How to build the service.
#[derive(Debug, Clone, Default)]
pub struct ServiceConfig {
    /// SQLite cache file; `None` uses the default location.
    pub cache: Option<PathBuf>,
    /// Do not use a cache at all.
    pub no_cache: bool,
    /// Extra folder of label packs.
    pub extra_labels: Option<PathBuf>,
}

pub struct AppService {
    provider: Box<dyn BitcoinProvider>,
    labels: RwLock<Arc<LabelIndex>>,
    config: ServiceConfig,
    cache_path: Option<PathBuf>,
}

impl AppService {
    pub fn new(config: ServiceConfig) -> Result<Self> {
        let live = ct_providers::default_bitcoin_provider()?;
        let (provider, cache_path): (Box<dyn BitcoinProvider>, _) = if config.no_cache {
            (Box::new(live), None)
        } else {
            let path = match &config.cache {
                Some(p) => p.clone(),
                None => ct_store::Store::default_path().ok_or_else(|| {
                    ServiceError::internal("no user data folder found; choose a cache file")
                })?,
            };
            let store = ct_store::Store::open(&path)?;
            let cached =
                ct_store::CachedBitcoin::new(live, store, ct_store::CachePolicy::default());
            (Box::new(cached), Some(path))
        };
        Self::with_provider(provider, config, cache_path)
    }

    /// Use a specific provider (tests, replay).
    pub fn with_provider(
        provider: Box<dyn BitcoinProvider>,
        config: ServiceConfig,
        cache_path: Option<PathBuf>,
    ) -> Result<Self> {
        let index = labels::load_index(config.extra_labels.as_deref())?;
        Ok(AppService {
            provider,
            labels: RwLock::new(Arc::new(index)),
            config,
            cache_path,
        })
    }

    fn labels(&self) -> Arc<LabelIndex> {
        self.labels
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn reload_labels(&self) -> Result<()> {
        let index = labels::load_index(self.config.extra_labels.as_deref())?;
        *self.labels.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(index);
        Ok(())
    }

    pub fn status(&self) -> StatusView {
        let labels = self.labels();
        StatusView {
            version: env!("CARGO_PKG_VERSION").to_string(),
            provider: self.provider.name().to_string(),
            cache: self.cache_path.as_ref().map(|p| p.display().to_string()),
            labelled_addresses: labels.len(),
            packs: labels.packs().len(),
            user_labels_dir: labels::user_labels_dir().map(|p| p.display().to_string()),
        }
    }

    /// Work out whether the user pasted an address or a transaction id.
    pub fn resolve(&self, req: ResolveRequest) -> Result<ResolveView> {
        let q = req.query.trim();
        if q.len() == 64 && q.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(ResolveView::Transaction {
                txid: q.to_ascii_lowercase(),
            });
        }
        let address = Address::detect(q).map_err(|e| {
            ServiceError::invalid(format!("Not a transaction id or a supported address ({e})"))
        })?;
        Ok(ResolveView::Address {
            chain: address.chain(),
            supported: address.chain() == Chain::Bitcoin,
            address: address.to_string(),
        })
    }

    pub async fn btc_address(&self, req: AddressRequest) -> Result<AddressView> {
        let address = Address::parse(Chain::Bitcoin, &req.address)?;
        let summary = self.provider.address_summary(&address).await?;
        let mut txs = Vec::new();
        for _ in 0..req.pages.clamp(1, 20) {
            let after = txs.last().map(|t: &ct_core::UtxoTx| t.txid.clone());
            let page = self
                .provider
                .address_txs(&address, after.as_deref())
                .await?;
            if page.is_empty() {
                break;
            }
            txs.extend(page);
        }
        let transactions = txs
            .iter()
            .map(|t| {
                let received: u64 = t
                    .outputs
                    .iter()
                    .filter(|o| o.address.as_ref() == Some(&address))
                    .map(|o| o.value)
                    .sum();
                let sent: u64 = t
                    .inputs
                    .iter()
                    .filter(|i| i.address.as_ref() == Some(&address))
                    .map(|i| i.value)
                    .sum();
                TxBrief {
                    txid: t.txid.clone(),
                    height: t.block.as_ref().map(|b| b.height),
                    time: t.block.as_ref().map(|b| b.time),
                    net: received as i64 - sent as i64,
                    fee: t.fee,
                    inputs: t.inputs.len(),
                    outputs: t.outputs.len(),
                }
            })
            .collect::<Vec<_>>();
        let labels = self.labels();
        Ok(AddressView {
            address: address.to_string(),
            chain: Chain::Bitcoin,
            balance: summary.balance() as u64,
            more: (transactions.len() as u64) < summary.tx_count,
            labels: labels.all(&address).into_iter().map(label_view).collect(),
            summary,
            transactions,
        })
    }

    pub async fn btc_tx(&self, req: TxRequest) -> Result<TxView> {
        let txid = normalize_txid(&req.txid)?;
        let tx = self.provider.tx(&txid).await?;
        let spends = self.provider.outspends(&txid).await?;
        let labels = self.labels();
        let label_of =
            |a: Option<&Address>| a.and_then(|a| labels.all(a).into_iter().next().map(label_view));
        let shape = classify(&tx);
        let change = detect_change(&tx);
        let peel = is_peel_step(&tx, change.as_ref());
        Ok(TxView {
            txid: tx.txid.clone(),
            block: tx.block.clone(),
            fee: tx.fee,
            vsize: tx.vsize,
            total_in: tx.total_in(),
            total_out: tx.total_out(),
            shape: shape_view(&shape),
            change: change.map(|c| ChangeView {
                output: c.output,
                confidence: c.confidence,
                evidence: c.evidence,
            }),
            peel,
            inputs: tx
                .inputs
                .iter()
                .map(|i| InputView {
                    address: i.address.as_ref().map(ToString::to_string),
                    value: i.value,
                    script_type: i.script_type.as_ref().map(script_name),
                    prev_txid: i.prev_txid.clone(),
                    prev_vout: i.prev_vout,
                    coinbase: i.is_coinbase,
                    label: label_of(i.address.as_ref()),
                })
                .collect(),
            outputs: tx
                .outputs
                .iter()
                .map(|o| {
                    let s = spends.get(o.index as usize);
                    OutputView {
                        index: o.index,
                        address: o.address.as_ref().map(ToString::to_string),
                        value: o.value,
                        script_type: script_name(&o.script_type),
                        label: label_of(o.address.as_ref()),
                        spendable: o.script_type.is_spendable(),
                        spent: s.is_some_and(|s| s.spent),
                        spent_by: s.and_then(|s| s.spending_txid.clone()),
                        spent_height: s.and_then(|s| s.block.as_ref().map(|b| b.height)),
                    }
                })
                .collect(),
        })
    }

    pub async fn btc_trace(
        &self,
        req: TraceRequest,
        progress: &mut (dyn FnMut(Progress) + Send),
    ) -> Result<TraceView> {
        let txid = normalize_txid(&req.txid)?;
        let seed = match (
            req.vout,
            req.to.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        ) {
            (Some(_), Some(_)) => {
                return Err(ServiceError::invalid(
                    "choose either an output number or a recipient address, not both",
                ));
            }
            (Some(vout), None) => Seed::Output { txid, vout },
            (None, Some(to)) => Seed::Payment {
                txid,
                address: Address::parse(Chain::Bitcoin, to)?,
            },
            (None, None) => Seed::Tx { txid },
        };
        let defaults = TraceConfig::default();
        let config = TraceConfig {
            direction: req.direction,
            max_hops: req.max_hops.unwrap_or(defaults.max_hops).clamp(1, 100),
            max_txs: req.max_txs.unwrap_or(defaults.max_txs).clamp(1, 5_000),
            min_amount: req.min_sats,
            check_addresses: req.check_addresses.unwrap_or(true),
            ..defaults
        };
        let labels = self.labels();
        let report = ct_trace::trace_bitcoin(
            self.provider.as_ref(),
            labels.as_ref(),
            seed,
            config,
            progress,
        )
        .await?;
        let graph = ct_trace::graph::build(&report);
        Ok(TraceView { report, graph })
    }

    pub fn label_lookup(&self, req: LabelLookupRequest) -> Result<Vec<LabelView>> {
        let address = Address::detect(&req.address)?;
        Ok(self
            .labels()
            .all(&address)
            .into_iter()
            .map(label_view)
            .collect())
    }

    pub fn label_packs(&self) -> Vec<PackView> {
        self.labels()
            .packs()
            .iter()
            .map(|p| PackView {
                id: p.meta.id.clone(),
                title: p.meta.title.clone(),
                source: p.meta.source.clone(),
                licence: p.meta.licence.clone(),
                as_of: p.meta.as_of.clone(),
                labels: p.labels,
                origin: p.origin.clone(),
                category: p.meta.category,
                confidence: p.meta.confidence,
            })
            .collect()
    }

    /// Add a label to the user's own pack and reload.
    pub fn label_add(&self, req: AddLabelRequest) -> Result<Vec<LabelView>> {
        let address = Address::detect(&req.address)?;
        let dir = labels::user_labels_dir()
            .ok_or_else(|| ServiceError::internal("no user data folder found"))?;
        labels::add_user_label(
            &dir,
            &address,
            &req.entity,
            req.category,
            req.confidence,
            req.note,
            req.source,
        )?;
        self.reload_labels()?;
        self.label_lookup(LabelLookupRequest {
            address: address.to_string(),
        })
    }

    /// Download the latest OFAC list into the user labels folder and reload.
    pub async fn update_ofac(&self) -> Result<OfacUpdateView> {
        let dir = labels::user_labels_dir()
            .ok_or_else(|| ServiceError::internal("no user data folder found"))?;
        let xml = labels::download_ofac().await?;
        let path = dir.join("ofac-sdn.toml");
        let (pack, stats) = labels::write_ofac_pack(&xml, &path)?;
        self.reload_labels()?;
        Ok(OfacUpdateView {
            as_of: pack.pack.as_of,
            addresses: stats.addresses,
            entries: stats.entries,
            unsupported: stats.unsupported,
            reassigned: stats.reassigned,
            invalid: stats.invalid,
            path: path.display().to_string(),
        })
    }
}

fn label_view(r: &LabelRecord) -> LabelView {
    LabelView {
        entity: r.entity.clone(),
        category: r.category,
        category_name: r.category.as_str().to_string(),
        confidence: r.confidence,
        pack: r.pack_id.clone(),
        source: r.source.clone(),
        as_of: r.as_of.clone(),
        note: r.note.clone(),
    }
}

fn script_name(s: &ScriptType) -> String {
    match s {
        ScriptType::P2pk => "p2pk".into(),
        ScriptType::P2pkh => "p2pkh".into(),
        ScriptType::P2sh => "p2sh".into(),
        ScriptType::P2wpkh => "p2wpkh".into(),
        ScriptType::P2wsh => "p2wsh".into(),
        ScriptType::P2tr => "p2tr".into(),
        ScriptType::Multisig => "multisig".into(),
        ScriptType::OpReturn => "op_return".into(),
        ScriptType::Other(o) => o.clone(),
    }
}

fn shape_view(shape: &TxShape) -> ShapeView {
    let (kind, coinjoin_confidence, evidence) = match shape {
        TxShape::Coinbase => ("coinbase", None, vec![]),
        TxShape::CoinJoin(i) => ("coinjoin", Some(i.confidence), i.evidence.clone()),
        TxShape::Consolidation { .. } => ("consolidation", None, vec![]),
        TxShape::BatchPayout { .. } => ("batch_payout", None, vec![]),
        TxShape::Simple => ("simple", None, vec![]),
    };
    ShapeView {
        kind: kind.into(),
        description: shape.describe(),
        service_confidence: shape.service_confidence(),
        coinjoin_confidence,
        evidence,
    }
}

/// Human-readable date for messages.
pub fn date(unix: u64) -> String {
    format_utc(unix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_core::LabelLookup;
    use ct_trace::testing::{MemoryBitcoin, addr, tx, txid};

    fn service() -> AppService {
        let t1 = tx(
            &txid(1),
            100,
            &[(&txid(900), 0, &addr(1), 100_010_000)],
            &[(&addr(2), 100_000_000)],
        );
        let t2 = tx(
            &txid(2),
            101,
            &[(&txid(1), 0, &addr(2), 100_000_000)],
            &[(&addr(3), 99_990_000)],
        );
        AppService::with_provider(
            Box::new(MemoryBitcoin::new([t1, t2])),
            ServiceConfig::default(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn resolves_queries() {
        let s = service();
        assert!(matches!(
            s.resolve(ResolveRequest { query: format!(" {} ", "A".repeat(64)) }).unwrap(),
            ResolveView::Transaction { txid } if txid == "a".repeat(64)
        ));
        assert!(matches!(
            s.resolve(ResolveRequest {
                query: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".into()
            })
            .unwrap(),
            ResolveView::Address {
                supported: false,
                ..
            }
        ));
        let err = s
            .resolve(ResolveRequest {
                query: "hello".into(),
            })
            .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(s.status().labelled_addresses > 40_000);
    }

    #[tokio::test]
    async fn tx_and_trace_views() {
        let s = service();
        let v = s.btc_tx(TxRequest { txid: txid(1) }).await.unwrap();
        assert_eq!(v.outputs.len(), 1);
        assert!(v.outputs[0].spent);
        assert_eq!(v.outputs[0].spent_by.as_deref(), Some(txid(2).as_str()));
        let t = s
            .btc_trace(
                TraceRequest {
                    txid: txid(1),
                    vout: Some(0),
                    to: None,
                    direction: ct_trace::Direction::Forward,
                    max_hops: None,
                    max_txs: None,
                    min_sats: Some(1),
                    check_addresses: Some(false),
                },
                &mut |_| {},
            )
            .await
            .unwrap();
        assert_eq!(t.report.leads.len(), 1);
        assert!(t.graph.nodes.len() >= 2);
        let bad = s
            .btc_trace(
                TraceRequest {
                    txid: "nope".into(),
                    vout: None,
                    to: None,
                    direction: ct_trace::Direction::Forward,
                    max_hops: None,
                    max_txs: None,
                    min_sats: None,
                    check_addresses: None,
                },
                &mut |_| {},
            )
            .await
            .unwrap_err();
        assert_eq!(bad.kind, ErrorKind::InvalidInput);
    }

    #[test]
    fn user_labels_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let a = Address::parse(Chain::Bitcoin, "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa").unwrap();
        let path = labels::add_user_label(
            dir.path(),
            &a,
            "Genesis",
            ct_core::EntityCategory::Individual,
            ct_core::Confidence::Low,
            Some("note".into()),
            None,
        )
        .unwrap();
        let mut index = LabelIndex::new();
        index.load_dir(path.parent().unwrap()).unwrap();
        assert_eq!(index.lookup(&a).unwrap().entity, "Genesis");
        assert!(
            labels::add_user_label(
                dir.path(),
                &a,
                "x",
                ct_core::EntityCategory::Individual,
                ct_core::Confidence::Certain,
                None,
                None
            )
            .is_err()
        );
    }
}
