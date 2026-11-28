//! Esplora REST API (mempool.space, blockstream.info, or a self-hosted electrs/Esplora).
//!
//! API reference: https://github.com/Blockstream/esplora/blob/master/API.md

use async_trait::async_trait;
use ct_core::{
    Address, AddressSummary, BlockRef, Chain, OutputSpend, ScriptType, TxInput, TxOutput, UtxoTx,
};
use serde::Deserialize;

use crate::bitcoin::{BitcoinProvider, normalize_txid, require_bitcoin};
use crate::error::{ProviderError, Result};
use crate::http::HttpClient;

pub struct Esplora {
    name: String,
    /// Base URL including the API prefix, without trailing slash,
    /// e.g. `https://mempool.space/api`.
    base: String,
    http: HttpClient,
}

impl Esplora {
    pub fn new(name: impl Into<String>, base: impl Into<String>, http: HttpClient) -> Self {
        let base = base.into().trim_end_matches('/').to_string();
        Esplora {
            name: name.into(),
            base,
            http,
        }
    }

    pub fn mempool_space(http: HttpClient) -> Self {
        Self::new("mempool.space", "https://mempool.space/api", http)
    }

    pub fn blockstream(http: HttpClient) -> Self {
        Self::new("blockstream.info", "https://blockstream.info/api", http)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.http
            .get_json(&self.name, &format!("{}{}", self.base, path))
            .await
    }

    fn bad(&self, detail: impl Into<String>) -> ProviderError {
        ProviderError::BadResponse {
            source_name: self.name.clone(),
            detail: detail.into(),
        }
    }
}

#[async_trait]
impl BitcoinProvider for Esplora {
    fn name(&self) -> &str {
        &self.name
    }

    async fn tip_height(&self) -> Result<u64> {
        let url = format!("{}/blocks/tip/height", self.base);
        let text = self.http.get_text(&self.name, &url).await?;
        text.trim()
            .parse()
            .map_err(|_| self.bad(format!("tip height not a number: {text}")))
    }

    async fn address_summary(&self, address: &Address) -> Result<AddressSummary> {
        require_bitcoin(address)?;
        let raw: EAddress = self.get(&format!("/address/{address}")).await?;
        Ok(raw.into_summary(address.clone()))
    }

    async fn address_txs(
        &self,
        address: &Address,
        after_txid: Option<&str>,
    ) -> Result<Vec<UtxoTx>> {
        require_bitcoin(address)?;
        let path = match after_txid {
            Some(t) => format!("/address/{address}/txs/chain/{}", normalize_txid(t)?),
            None => format!("/address/{address}/txs/chain"),
        };
        let raw: Vec<ETx> = self.get(&path).await?;
        raw.into_iter()
            .map(|t| t.into_tx().map_err(|e| self.bad(e)))
            .collect()
    }

    async fn tx(&self, txid: &str) -> Result<UtxoTx> {
        let txid = normalize_txid(txid)?;
        let raw: ETx = self.get(&format!("/tx/{txid}")).await?;
        raw.into_tx().map_err(|e| self.bad(e))
    }

    async fn outspends(&self, txid: &str) -> Result<Vec<OutputSpend>> {
        let txid = normalize_txid(txid)?;
        let raw: Vec<EOutspend> = self.get(&format!("/tx/{txid}/outspends")).await?;
        Ok(raw.into_iter().map(EOutspend::into_spend).collect())
    }
}

/// Parse a raw Esplora `/tx/:txid` JSON document. Used to load saved fixtures in tests.
pub fn parse_tx_json(json: &str) -> std::result::Result<UtxoTx, String> {
    serde_json::from_str::<ETx>(json)
        .map_err(|e| e.to_string())?
        .into_tx()
}

/// Parse a raw Esplora `/tx/:txid/outspends` JSON document.
pub fn parse_outspends_json(json: &str) -> std::result::Result<Vec<OutputSpend>, String> {
    serde_json::from_str::<Vec<EOutspend>>(json)
        .map(|v| v.into_iter().map(EOutspend::into_spend).collect())
        .map_err(|e| e.to_string())
}

// ---- Esplora JSON shapes ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct EStatus {
    confirmed: bool,
    block_height: Option<u64>,
    block_hash: Option<String>,
    block_time: Option<u64>,
}

impl EStatus {
    fn block(&self) -> Option<BlockRef> {
        if !self.confirmed {
            return None;
        }
        Some(BlockRef {
            height: self.block_height?,
            hash: self.block_hash.clone()?,
            time: self.block_time?,
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct EVout {
    scriptpubkey: String,
    scriptpubkey_type: String,
    scriptpubkey_address: Option<String>,
    value: u64,
}

impl EVout {
    fn script_type(&self) -> ScriptType {
        match self.scriptpubkey_type.as_str() {
            "p2pk" => ScriptType::P2pk,
            "p2pkh" => ScriptType::P2pkh,
            "p2sh" => ScriptType::P2sh,
            "v0_p2wpkh" => ScriptType::P2wpkh,
            "v0_p2wsh" => ScriptType::P2wsh,
            "v1_p2tr" => ScriptType::P2tr,
            "multisig" => ScriptType::Multisig,
            "op_return" => ScriptType::OpReturn,
            other => ScriptType::Other(other.to_string()),
        }
    }

    fn address(&self) -> Option<Address> {
        let raw = self.scriptpubkey_address.as_deref()?;
        match Address::parse(Chain::Bitcoin, raw) {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::warn!(address = raw, error = %e, "unparseable output address");
                None
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct EVin {
    txid: String,
    vout: u32,
    prevout: Option<EVout>,
    is_coinbase: bool,
    sequence: u32,
    #[serde(default)]
    inner_witnessscript_asm: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ETx {
    txid: String,
    version: u32,
    locktime: u32,
    vin: Vec<EVin>,
    vout: Vec<EVout>,
    weight: u32,
    fee: u64,
    status: EStatus,
}

impl ETx {
    pub(crate) fn into_tx(self) -> std::result::Result<UtxoTx, String> {
        let txid = normalize_txid(&self.txid).map_err(|e| e.to_string())?;
        let inputs = self
            .vin
            .into_iter()
            .map(|i| {
                if i.is_coinbase {
                    return Ok(TxInput {
                        prev_txid: None,
                        prev_vout: i.vout,
                        address: None,
                        value: 0,
                        script_type: None,
                        is_coinbase: true,
                        sequence: i.sequence,
                        witness_script: None,
                    });
                }
                let prev = i
                    .prevout
                    .ok_or_else(|| format!("input {}:{} has no prevout", i.txid, i.vout))?;
                Ok(TxInput {
                    prev_txid: Some(i.txid),
                    prev_vout: i.vout,
                    address: prev.address(),
                    value: prev.value,
                    script_type: Some(prev.script_type()),
                    is_coinbase: false,
                    sequence: i.sequence,
                    witness_script: i.inner_witnessscript_asm,
                })
            })
            .collect::<std::result::Result<Vec<_>, String>>()?;
        let outputs = self
            .vout
            .into_iter()
            .enumerate()
            .map(|(n, o)| TxOutput {
                index: n as u32,
                address: o.address(),
                value: o.value,
                script_type: o.script_type(),
                script_pubkey: o.scriptpubkey,
            })
            .collect();
        Ok(UtxoTx {
            chain: Chain::Bitcoin,
            txid,
            block: self.status.block(),
            version: self.version,
            locktime: self.locktime,
            inputs,
            outputs,
            fee: self.fee,
            vsize: self.weight.div_ceil(4),
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct EOutspend {
    spent: bool,
    txid: Option<String>,
    vin: Option<u32>,
    status: Option<EStatus>,
}

impl EOutspend {
    fn into_spend(self) -> OutputSpend {
        OutputSpend {
            spent: self.spent,
            spending_txid: self.txid,
            spending_vin: self.vin,
            block: self.status.as_ref().and_then(EStatus::block),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct EStats {
    funded_txo_count: u64,
    funded_txo_sum: u64,
    spent_txo_count: u64,
    spent_txo_sum: u64,
    tx_count: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EAddress {
    chain_stats: EStats,
    mempool_stats: EStats,
}

impl EAddress {
    fn into_summary(self, address: Address) -> AddressSummary {
        let c = self.chain_stats;
        let m = self.mempool_stats;
        AddressSummary {
            address,
            tx_count: c.tx_count,
            received_outputs: c.funded_txo_count,
            received: c.funded_txo_sum as u128,
            spent_outputs: c.spent_txo_count,
            spent: c.spent_txo_sum as u128,
            unconfirmed_tx_count: m.tx_count,
            unconfirmed_delta: m.funded_txo_sum as i128 - m.spent_txo_sum as i128,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/tests/fixtures/esplora/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    fn parse_tx(name: &str) -> UtxoTx {
        serde_json::from_str::<ETx>(&fixture(name))
            .unwrap()
            .into_tx()
            .unwrap()
    }

    #[test]
    fn first_bitcoin_payment_p2pk() {
        // Block 170: first person-to-person payment (to Hal Finney).
        let tx = parse_tx("tx_p2pk_block170.json");
        assert_eq!(
            tx.txid,
            "f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16"
        );
        assert_eq!(tx.block.as_ref().unwrap().height, 170);
        assert_eq!(tx.block.as_ref().unwrap().time, 1_231_731_025);
        assert_eq!(tx.inputs.len(), 1);
        assert_eq!(tx.total_in(), 5_000_000_000);
        assert_eq!(
            tx.outputs.iter().map(|o| o.value).collect::<Vec<_>>(),
            vec![1_000_000_000, 4_000_000_000]
        );
        // P2PK outputs have no address; the script identifies them.
        assert!(tx.outputs.iter().all(|o| o.address.is_none()));
        assert!(tx.outputs.iter().all(|o| o.script_type == ScriptType::P2pk));
        assert!(!tx.outputs[0].script_pubkey.is_empty());
        assert_eq!(tx.fee, 0);
        assert!(!tx.is_coinbase());
    }

    #[test]
    fn coinbase_input() {
        let tx = parse_tx("tx_coinbase_block1.json");
        assert!(tx.is_coinbase());
        assert_eq!(tx.inputs[0].prev_txid, None);
        assert_eq!(tx.total_out(), 5_000_000_000);
        assert_eq!(tx.block.unwrap().height, 1);
    }

    #[test]
    fn modern_segwit_and_taproot() {
        let tx = parse_tx("tx_segwit_taproot.json");
        assert_eq!(tx.inputs.len(), 2);
        assert_eq!(tx.outputs.len(), 2);
        assert_eq!(tx.outputs[0].script_type, ScriptType::P2tr);
        assert_eq!(tx.outputs[1].script_type, ScriptType::P2wpkh);
        assert!(tx.outputs.iter().all(|o| o.address.is_some()));
        assert!(tx.inputs.iter().all(|i| i.address.is_some()));
        // Value is conserved: inputs = outputs + fee.
        assert_eq!(tx.total_in(), tx.total_out() + tx.fee);
    }

    #[test]
    fn outspends_spent_and_unspent() {
        let raw: Vec<EOutspend> =
            serde_json::from_str(&fixture("outspends_segwit_taproot.json")).unwrap();
        let spends: Vec<OutputSpend> = raw.into_iter().map(EOutspend::into_spend).collect();
        assert_eq!(spends.len(), 2);
        assert!(spends[0].spent);
        assert_eq!(spends[0].spending_vin, Some(0));
        assert!(
            spends[0]
                .spending_txid
                .as_deref()
                .unwrap()
                .starts_with("67561eac")
        );
        assert!(spends[0].block.is_some());
        assert!(!spends[1].spent);
        assert_eq!(spends[1].spending_txid, None);
    }

    #[test]
    fn early_outspends() {
        let raw: Vec<EOutspend> =
            serde_json::from_str(&fixture("outspends_block170.json")).unwrap();
        let spends: Vec<OutputSpend> = raw.into_iter().map(EOutspend::into_spend).collect();
        // Hal's output was spent in block 92240; Satoshi's change in block 181.
        assert_eq!(spends[0].block.as_ref().unwrap().height, 92_240);
        assert_eq!(spends[1].block.as_ref().unwrap().height, 181);
    }

    #[test]
    fn address_stats() {
        let raw: EAddress = serde_json::from_str(&fixture("address_stats.json")).unwrap();
        let addr = Address::parse(Chain::Bitcoin, "12cbQLTFMXRnSzktFkuoG3eHoMeFtpTu3S").unwrap();
        let s = raw.into_summary(addr);
        assert_eq!(s.tx_count, 134);
        assert_eq!(s.received, 44_411_447);
        assert_eq!(s.balance(), 44_411_447);
    }

    #[test]
    fn address_txs_page() {
        let raw: Vec<ETx> = serde_json::from_str(&fixture("address_txs_page.json")).unwrap();
        let txs: Vec<UtxoTx> = raw.into_iter().map(|t| t.into_tx().unwrap()).collect();
        assert_eq!(txs.len(), 2);
        assert!(txs.iter().all(|t| t.block.is_some()));
    }

    /// Live check against mempool.space. Run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_mempool_space() {
        let http = HttpClient::new(Default::default()).unwrap();
        let api = Esplora::mempool_space(http);
        assert!(api.tip_height().await.unwrap() > 900_000);
        let tx = api
            .tx("f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16")
            .await
            .unwrap();
        assert_eq!(tx.block.unwrap().height, 170);
        let spends = api.outspends(&tx.txid).await.unwrap();
        assert!(spends.iter().all(|s| s.spent));
        let missing = api.tx(&"0".repeat(64)).await;
        assert!(
            matches!(missing, Err(ProviderError::NotFound(_))),
            "{missing:?}"
        );
    }
}
