//! In-memory Bitcoin provider and transaction builders, for tests of the engine and of
//! anything built on it. Never used with real data.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use ct_core::{
    Address, AddressSummary, BlockRef, Chain, OutputSpend, ScriptType, TxInput, TxOutput, UtxoTx,
};
use ct_providers::{BitcoinProvider, ProviderError};

/// Deterministic fake address number `n`.
pub fn addr(n: u8) -> Address {
    Address::bitcoin_p2pkh([n; 20])
}

/// Deterministic fake txid number `n`.
pub fn txid(n: u32) -> String {
    format!("{n:064x}")
}

/// Builds a confirmed P2PKH transaction. Inputs: `(prev_txid, prev_vout, address, value)`.
/// Outputs: `(address, value)`.
pub fn tx(
    id: &str,
    height: u64,
    inputs: &[(&str, u32, &Address, u64)],
    outputs: &[(&Address, u64)],
) -> UtxoTx {
    let total_in: u64 = inputs.iter().map(|i| i.3).sum();
    let total_out: u64 = outputs.iter().map(|o| o.1).sum();
    UtxoTx {
        chain: Chain::Bitcoin,
        txid: id.to_string(),
        block: Some(BlockRef {
            height,
            hash: format!("{height:064x}"),
            time: 1_700_000_000 + height * 600,
        }),
        version: 2,
        locktime: 0,
        inputs: inputs
            .iter()
            .map(|(prev, vout, a, v)| TxInput {
                prev_txid: Some(prev.to_string()),
                prev_vout: *vout,
                address: Some((*a).clone()),
                value: *v,
                script_type: Some(ScriptType::P2pkh),
                is_coinbase: false,
                sequence: u32::MAX,
            })
            .collect(),
        outputs: outputs
            .iter()
            .enumerate()
            .map(|(n, (a, v))| TxOutput {
                index: n as u32,
                address: Some((*a).clone()),
                value: *v,
                script_type: ScriptType::P2pkh,
                script_pubkey: String::new(),
            })
            .collect(),
        fee: total_in.saturating_sub(total_out),
        vsize: 200,
    }
}

/// A coinbase transaction paying `value` to `to`.
pub fn coinbase(id: &str, height: u64, to: &Address, value: u64) -> UtxoTx {
    let mut t = tx(id, height, &[], &[(to, value)]);
    t.inputs.push(TxInput {
        prev_txid: None,
        prev_vout: u32::MAX,
        address: None,
        value: 0,
        script_type: None,
        is_coinbase: true,
        sequence: u32::MAX,
    });
    t.fee = 0;
    t
}

/// Serves transactions from memory. Spends are derived from the transactions added, so a
/// graph is built just by adding its transactions.
#[derive(Default)]
pub struct MemoryBitcoin {
    txs: HashMap<String, UtxoTx>,
    summaries: HashMap<Address, AddressSummary>,
    /// Calls per method, for budget assertions.
    pub calls: Mutex<HashMap<&'static str, u32>>,
}

impl MemoryBitcoin {
    pub fn new(txs: impl IntoIterator<Item = UtxoTx>) -> Self {
        MemoryBitcoin {
            txs: txs.into_iter().map(|t| (t.txid.clone(), t)).collect(),
            ..Default::default()
        }
    }

    pub fn with_summary(mut self, address: &Address, tx_count: u64, spent_outputs: u64) -> Self {
        self.summaries.insert(
            address.clone(),
            AddressSummary {
                address: address.clone(),
                tx_count,
                received_outputs: tx_count,
                received: 0,
                spent_outputs,
                spent: 0,
                unconfirmed_tx_count: 0,
                unconfirmed_delta: 0,
            },
        );
        self
    }

    fn count(&self, method: &'static str) {
        *self.calls.lock().unwrap().entry(method).or_default() += 1;
    }

    pub fn call_count(&self, method: &str) -> u32 {
        self.calls.lock().unwrap().get(method).copied().unwrap_or(0)
    }
}

#[async_trait]
impl BitcoinProvider for MemoryBitcoin {
    fn name(&self) -> &str {
        "memory"
    }

    async fn tip_height(&self) -> Result<u64, ProviderError> {
        Ok(self
            .txs
            .values()
            .filter_map(|t| t.block.as_ref().map(|b| b.height))
            .max()
            .unwrap_or(0))
    }

    async fn address_summary(&self, address: &Address) -> Result<AddressSummary, ProviderError> {
        self.count("address_summary");
        Ok(self
            .summaries
            .get(address)
            .cloned()
            .unwrap_or(AddressSummary {
                address: address.clone(),
                tx_count: 2,
                received_outputs: 1,
                received: 0,
                spent_outputs: 1,
                spent: 0,
                unconfirmed_tx_count: 0,
                unconfirmed_delta: 0,
            }))
    }

    async fn address_txs(
        &self,
        _address: &Address,
        _after_txid: Option<&str>,
    ) -> Result<Vec<UtxoTx>, ProviderError> {
        Err(ProviderError::Unsupported("memory".into()))
    }

    async fn tx(&self, txid: &str) -> Result<UtxoTx, ProviderError> {
        self.count("tx");
        self.txs
            .get(txid)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound(txid.to_string()))
    }

    async fn outspends(&self, txid: &str) -> Result<Vec<OutputSpend>, ProviderError> {
        self.count("outspends");
        let tx = self
            .txs
            .get(txid)
            .ok_or_else(|| ProviderError::NotFound(txid.to_string()))?;
        Ok(tx
            .outputs
            .iter()
            .map(|o| {
                let spender = self.txs.values().find_map(|t| {
                    t.inputs
                        .iter()
                        .position(|i| {
                            i.prev_txid.as_deref() == Some(txid) && i.prev_vout == o.index
                        })
                        .map(|vin| (t, vin as u32))
                });
                match spender {
                    Some((t, vin)) => OutputSpend {
                        spent: true,
                        spending_txid: Some(t.txid.clone()),
                        spending_vin: Some(vin),
                        block: t.block.clone(),
                    },
                    None => OutputSpend {
                        spent: false,
                        spending_txid: None,
                        spending_vin: None,
                        block: None,
                    },
                }
            })
            .collect())
    }
}

/// Everything a trace fetched, so it can be replayed offline (golden tests, bug reports).
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Recording {
    pub txs: std::collections::BTreeMap<String, UtxoTx>,
    pub outspends: std::collections::BTreeMap<String, Vec<OutputSpend>>,
    /// Keyed by address string.
    pub summaries: std::collections::BTreeMap<String, AddressSummary>,
}

/// Wraps a provider and records every successful answer.
pub struct Recorder<'a> {
    inner: &'a dyn BitcoinProvider,
    recording: Mutex<Recording>,
}

impl<'a> Recorder<'a> {
    pub fn new(inner: &'a dyn BitcoinProvider) -> Self {
        Recorder {
            inner,
            recording: Mutex::new(Recording::default()),
        }
    }

    pub fn into_recording(self) -> Recording {
        self.recording.into_inner().unwrap()
    }
}

#[async_trait]
impl BitcoinProvider for Recorder<'_> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn tip_height(&self) -> Result<u64, ProviderError> {
        self.inner.tip_height().await
    }

    async fn address_summary(&self, address: &Address) -> Result<AddressSummary, ProviderError> {
        let s = self.inner.address_summary(address).await?;
        self.recording
            .lock()
            .unwrap()
            .summaries
            .insert(address.as_str().to_string(), s.clone());
        Ok(s)
    }

    async fn address_txs(
        &self,
        address: &Address,
        after_txid: Option<&str>,
    ) -> Result<Vec<UtxoTx>, ProviderError> {
        self.inner.address_txs(address, after_txid).await
    }

    async fn tx(&self, txid: &str) -> Result<UtxoTx, ProviderError> {
        let t = self.inner.tx(txid).await?;
        self.recording
            .lock()
            .unwrap()
            .txs
            .insert(txid.to_string(), t.clone());
        Ok(t)
    }

    async fn outspends(&self, txid: &str) -> Result<Vec<OutputSpend>, ProviderError> {
        let s = self.inner.outspends(txid).await?;
        self.recording
            .lock()
            .unwrap()
            .outspends
            .insert(txid.to_string(), s.clone());
        Ok(s)
    }
}

/// Serves exactly what a [`Recording`] holds; anything else is "not found".
pub struct Replay {
    recording: Recording,
}

impl Replay {
    pub fn new(recording: Recording) -> Self {
        Replay { recording }
    }
}

#[async_trait]
impl BitcoinProvider for Replay {
    fn name(&self) -> &str {
        "replay"
    }

    async fn tip_height(&self) -> Result<u64, ProviderError> {
        Ok(self
            .recording
            .txs
            .values()
            .filter_map(|t| t.block.as_ref().map(|b| b.height))
            .max()
            .unwrap_or(0))
    }

    async fn address_summary(&self, address: &Address) -> Result<AddressSummary, ProviderError> {
        self.recording
            .summaries
            .get(address.as_str())
            .cloned()
            .ok_or_else(|| ProviderError::NotFound(address.to_string()))
    }

    async fn address_txs(
        &self,
        _address: &Address,
        _after_txid: Option<&str>,
    ) -> Result<Vec<UtxoTx>, ProviderError> {
        Err(ProviderError::Unsupported("replay".into()))
    }

    async fn tx(&self, txid: &str) -> Result<UtxoTx, ProviderError> {
        self.recording
            .txs
            .get(txid)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound(txid.to_string()))
    }

    async fn outspends(&self, txid: &str) -> Result<Vec<OutputSpend>, ProviderError> {
        self.recording
            .outspends
            .get(txid)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound(txid.to_string()))
    }
}
