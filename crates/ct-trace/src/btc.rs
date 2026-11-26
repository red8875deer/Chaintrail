//! Bitcoin tracing.
//!
//! Value is followed largest-first, so a limited budget is spent on the biggest flows.
//! Allocation is pro-rata (`docs/TRACING.md` §5): a transaction's traced share of its
//! inputs is passed to each output in proportion to the output's value. Fees and rounding
//! are recorded, so leads + fees + dust always add up to the seed amount.

use std::collections::HashMap;

use ct_core::{Address, AddressSummary, Chain, Claim, EntityCategory, OutputSpend, UtxoTx};
use ct_heuristics::btc::{TxShape, classify, detect_change, is_peel_step, service_from_summary};
use ct_providers::{BitcoinProvider, ProviderError};
use serde::{Deserialize, Serialize};

use crate::labels::LabelLookup;
use crate::report::{
    Direction, Flow, FlowRole, Lead, LeadKind, Progress, StopReason, TraceConfig, TraceReport,
    TraceStats,
};

/// Where a trace starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "seed", rename_all = "snake_case")]
pub enum Seed {
    /// One output of a transaction.
    Output { txid: String, vout: u32 },
    /// The outputs of `txid` paying `address` (e.g. a victim's payment to a scammer).
    Payment { txid: String, address: Address },
    /// The whole transaction: all outputs (forward) or all inputs (backward).
    Tx { txid: String },
}

impl Seed {
    pub fn txid(&self) -> &str {
        match self {
            Seed::Output { txid, .. } | Seed::Payment { txid, .. } | Seed::Tx { txid } => txid,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Seed::Output { txid, vout } => format!("{txid}:{vout}"),
            Seed::Payment { txid, address } => format!("{txid} → {address}"),
            Seed::Tx { txid } => txid.clone(),
        }
    }
}

/// Run a trace. Fails only if the seed itself cannot be loaded; later failures become
/// "unexplored" leads.
pub async fn trace_bitcoin(
    provider: &dyn BitcoinProvider,
    labels: &dyn LabelLookup,
    seed: Seed,
    config: TraceConfig,
    progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<TraceReport, ProviderError> {
    let mut t = Tracer {
        provider,
        labels,
        config: config.clone(),
        txs: HashMap::new(),
        spends: HashMap::new(),
        summaries: HashMap::new(),
        leads: Vec::new(),
        lead_index: HashMap::new(),
        flows: Vec::new(),
        stats: TraceStats::default(),
    };
    let seed_tx = t.tx(seed.txid()).await?;
    let seed_outputs: Vec<(u32, Option<Address>, u64)> = match &seed {
        Seed::Output { vout, .. } => {
            let o = seed_tx
                .outputs
                .iter()
                .find(|o| o.index == *vout)
                .ok_or_else(|| {
                    ProviderError::InvalidInput(format!(
                        "transaction {} has no output {vout}",
                        seed_tx.txid
                    ))
                })?;
            vec![(o.index, o.address.clone(), o.value)]
        }
        Seed::Payment { address, .. } => {
            let outs: Vec<_> = seed_tx
                .outputs
                .iter()
                .filter(|o| o.address.as_ref() == Some(address))
                .map(|o| (o.index, o.address.clone(), o.value))
                .collect();
            if outs.is_empty() {
                return Err(ProviderError::InvalidInput(format!(
                    "transaction {} does not pay {address}",
                    seed_tx.txid
                )));
            }
            outs
        }
        Seed::Tx { .. } => seed_tx
            .outputs
            .iter()
            .filter(|o| o.script_type.is_spendable() && o.value > 0)
            .map(|o| (o.index, o.address.clone(), o.value))
            .collect(),
    };
    let seed_amount: u64 = match (config.direction, &seed) {
        (Direction::Backward, Seed::Tx { .. }) => seed_tx.total_out(),
        _ => seed_outputs.iter().map(|(_, _, v)| v).sum(),
    };
    let min_amount = config
        .min_amount
        .unwrap_or_else(|| (seed_amount / 1_000).max(1_000));

    match config.direction {
        Direction::Forward => {
            let coins = seed_outputs
                .into_iter()
                .map(|(vout, address, value)| Coin {
                    txid: seed_tx.txid.clone(),
                    vout,
                    address,
                    amount: value,
                    hop: 0,
                    time: block_time(&seed_tx),
                    path: vec![seed_tx.txid.clone()],
                })
                .collect();
            t.forward(coins, min_amount, progress).await;
        }
        Direction::Backward => {
            let receiver = seed_outputs.first().and_then(|(_, a, _)| a.clone());
            let source = Source {
                txid: seed_tx.txid.clone(),
                amount: seed_amount,
                hop: 0,
                receiver,
                path: vec![seed_tx.txid.clone()],
            };
            t.backward(vec![source], min_amount, progress).await;
        }
    }

    let mut leads = t.leads;
    leads.sort_by(|a, b| b.amount.cmp(&a.amount).then(a.hops.cmp(&b.hops)));
    Ok(TraceReport {
        direction: config.direction,
        seed: seed.describe(),
        seed_amount,
        allocation: "pro_rata".into(),
        min_amount,
        config,
        leads,
        flows: t.flows,
        stats: t.stats,
    })
}

/// A traced amount sitting in one transaction output (forward).
#[derive(Debug, Clone)]
struct Coin {
    txid: String,
    vout: u32,
    address: Option<Address>,
    amount: u64,
    hop: u32,
    time: Option<u64>,
    path: Vec<String>,
}

/// A traced amount to explain by the inputs of `txid` (backward).
#[derive(Debug, Clone)]
struct Source {
    txid: String,
    amount: u64,
    hop: u32,
    /// Address that received the traced funds in `txid`.
    receiver: Option<Address>,
    path: Vec<String>,
}

struct Tracer<'a> {
    provider: &'a dyn BitcoinProvider,
    labels: &'a dyn LabelLookup,
    config: TraceConfig,
    txs: HashMap<String, UtxoTx>,
    spends: HashMap<String, Vec<OutputSpend>>,
    summaries: HashMap<Address, Option<AddressSummary>>,
    leads: Vec<Lead>,
    lead_index: HashMap<String, usize>,
    flows: Vec<Flow>,
    stats: TraceStats,
}

fn block_time(tx: &UtxoTx) -> Option<u64> {
    tx.block.as_ref().map(|b| b.time)
}

/// Pop the pending entry with the largest amount (ties: lowest key, for determinism).
fn pop_largest<T>(pending: &mut HashMap<String, T>, amount: impl Fn(&T) -> u64) -> Option<T> {
    let key = pending
        .iter()
        .max_by(|a, b| amount(a.1).cmp(&amount(b.1)).then(b.0.cmp(a.0)))
        .map(|(k, _)| k.clone())?;
    pending.remove(&key)
}

impl<'a> Tracer<'a> {
    async fn tx(&mut self, txid: &str) -> Result<UtxoTx, ProviderError> {
        if let Some(tx) = self.txs.get(txid) {
            return Ok(tx.clone());
        }
        let tx = self.provider.tx(txid).await?;
        self.stats.txs_examined += 1;
        self.txs.insert(txid.to_string(), tx.clone());
        Ok(tx)
    }

    fn budget_left(&self, txid: &str) -> bool {
        self.txs.contains_key(txid) || (self.txs.len() as u32) < self.config.max_txs
    }

    async fn outspend(&mut self, txid: &str, vout: u32) -> Result<OutputSpend, ProviderError> {
        if !self.spends.contains_key(txid) {
            let s = self.provider.outspends(txid).await?;
            self.spends.insert(txid.to_string(), s);
        }
        self.spends[txid]
            .get(vout as usize)
            .cloned()
            .ok_or_else(|| ProviderError::BadResponse {
                source_name: self.provider.name().to_string(),
                detail: format!("no spend record for {txid}:{vout}"),
            })
    }

    /// Endpoint check for an address: labels first, then behaviour.
    async fn endpoint_for(&mut self, address: &Address) -> Option<(String, EntityCategory, Claim)> {
        if let Some(label) = self.labels.lookup(address) {
            let title = if label.entity.is_empty() {
                format!("{:?}", label.category)
            } else {
                label.entity.clone()
            };
            return Some((title, label.category, label.claim));
        }
        if !self.config.check_addresses {
            return None;
        }
        if !self.summaries.contains_key(address) {
            self.stats.addresses_checked += 1;
            let s = match self.provider.address_summary(address).await {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!(%address, error = %e, "address check failed");
                    None
                }
            };
            self.summaries.insert(address.clone(), s);
        }
        let hint = self.summaries[address]
            .as_ref()
            .and_then(service_from_summary)
            .filter(|h| h.confidence >= self.config.stop_confidence)?;
        Some((
            "Likely service (busy wallet)".into(),
            hint.category,
            Claim::Inference {
                heuristic: "service.busy_address".into(),
                confidence: hint.confidence,
                evidence: hint.evidence,
            },
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn add_lead(
        &mut self,
        kind: LeadKind,
        title: String,
        category: Option<EntityCategory>,
        address: Option<Address>,
        txid: Option<String>,
        amount: u64,
        time: Option<u64>,
        hops: u32,
        claim: Claim,
        path: &[String],
    ) {
        let tag = match &kind {
            LeadKind::Endpoint => "endpoint".to_string(),
            LeadKind::AtRest => "at_rest".to_string(),
            LeadKind::Mined => "mined".to_string(),
            LeadKind::Unexplored { stop } => format!("unexplored:{stop:?}"),
        };
        let place = match (&address, &txid) {
            (Some(a), _) => a.to_string(),
            (None, Some(t)) => t.clone(),
            (None, None) => String::new(),
        };
        let key = format!("{tag}|{place}|{title}");
        if let Some(&i) = self.lead_index.get(&key) {
            let lead = &mut self.leads[i];
            lead.amount += amount;
            lead.hops = lead.hops.min(hops);
            lead.first_time = match (lead.first_time, time) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            return;
        }
        self.lead_index.insert(key, self.leads.len());
        self.leads.push(Lead {
            kind,
            title,
            category,
            address,
            txid,
            amount,
            first_time: time,
            hops,
            // Paths are on-chain facts under the allocation rule, so the lead's confidence
            // is its own claim's.
            confidence: claim.confidence(),
            claim,
            path: path.to_vec(),
        });
    }

    fn unexplored(
        &mut self,
        stop: StopReason,
        address: Option<Address>,
        txid: &str,
        amount: u64,
        hop: u32,
        path: &[String],
    ) {
        let title = match &stop {
            StopReason::HopLimit => "Not followed: hop limit reached".to_string(),
            StopReason::Budget => "Not followed: transaction budget used up".to_string(),
            StopReason::Error(e) => format!("Not followed: data error ({e})"),
        };
        if stop == StopReason::Budget {
            self.stats.budget_exhausted = true;
        }
        self.add_lead(
            LeadKind::Unexplored { stop },
            title,
            None,
            address,
            Some(txid.to_string()),
            amount,
            None,
            hop,
            Claim::Fact,
            path,
        );
    }

    async fn forward(
        &mut self,
        seeds: Vec<Coin>,
        min_amount: u64,
        progress: &mut (dyn FnMut(Progress) + Send),
    ) {
        let mut pending: HashMap<String, Coin> = HashMap::new();
        let push = |pending: &mut HashMap<String, Coin>, c: Coin| {
            pending
                .entry(format!("{}:{}", c.txid, c.vout))
                .and_modify(|e| {
                    e.amount += c.amount;
                    e.hop = e.hop.min(c.hop);
                })
                .or_insert(c);
        };
        for c in seeds {
            push(&mut pending, c);
        }

        while let Some(coin) = pop_largest(&mut pending, |c| c.amount) {
            progress(Progress {
                txs_examined: self.stats.txs_examined,
                pending: pending.len(),
                leads: self.leads.len(),
            });
            if coin.amount < min_amount {
                self.stats.below_threshold += coin.amount;
                continue;
            }
            // Endpoint at the address holding the coin (not for the seed itself: the user
            // chose to trace from there).
            if coin.hop > 0
                && let Some(addr) = coin.address.clone()
                && let Some((title, category, claim)) = self.endpoint_for(&addr).await
            {
                self.add_lead(
                    LeadKind::Endpoint,
                    title,
                    Some(category),
                    Some(addr),
                    Some(coin.txid.clone()),
                    coin.amount,
                    coin.time,
                    coin.hop,
                    claim,
                    &coin.path,
                );
                continue;
            }
            let spend = match self.outspend(&coin.txid, coin.vout).await {
                Ok(s) => s,
                Err(e) => {
                    self.unexplored(
                        StopReason::Error(e.to_string()),
                        coin.address.clone(),
                        &coin.txid,
                        coin.amount,
                        coin.hop,
                        &coin.path,
                    );
                    continue;
                }
            };
            let Some(spending_txid) = spend.spending_txid.clone().filter(|_| spend.spent) else {
                self.add_lead(
                    LeadKind::AtRest,
                    "Funds at rest (unspent)".into(),
                    None,
                    coin.address.clone(),
                    Some(coin.txid.clone()),
                    coin.amount,
                    coin.time,
                    coin.hop,
                    Claim::Fact,
                    &coin.path,
                );
                continue;
            };
            if coin.hop >= self.config.max_hops {
                self.unexplored(
                    StopReason::HopLimit,
                    coin.address.clone(),
                    &coin.txid,
                    coin.amount,
                    coin.hop,
                    &coin.path,
                );
                continue;
            }
            if !self.budget_left(&spending_txid) {
                self.unexplored(
                    StopReason::Budget,
                    coin.address.clone(),
                    &coin.txid,
                    coin.amount,
                    coin.hop,
                    &coin.path,
                );
                continue;
            }
            let stx = match self.tx(&spending_txid).await {
                Ok(t) => t,
                Err(e) => {
                    self.unexplored(
                        StopReason::Error(e.to_string()),
                        coin.address.clone(),
                        &coin.txid,
                        coin.amount,
                        coin.hop,
                        &coin.path,
                    );
                    continue;
                }
            };
            let mut path = coin.path.clone();
            path.push(stx.txid.clone());
            let time = block_time(&stx);
            let hop = coin.hop + 1;

            let shape = classify(&stx);
            if let TxShape::CoinJoin(info) = &shape {
                self.add_lead(
                    LeadKind::Endpoint,
                    format!("Mixed in {}", info.kind.label()),
                    Some(EntityCategory::Mixer),
                    None,
                    Some(stx.txid.clone()),
                    coin.amount,
                    time,
                    hop,
                    Claim::Inference {
                        heuristic: "coinjoin".into(),
                        confidence: info.confidence,
                        evidence: info.evidence.clone(),
                    },
                    &path,
                );
                continue;
            }
            if let Some(conf) = shape.service_confidence()
                && conf >= self.config.stop_confidence
            {
                let title = match &shape {
                    TxShape::Consolidation {
                        distinct_inputs, ..
                    } => format!(
                        "Likely service: deposit swept together with {} other addresses",
                        distinct_inputs.saturating_sub(1)
                    ),
                    _ => format!("Likely service: wallet paid out in a {}", shape.describe()),
                };
                self.add_lead(
                    LeadKind::Endpoint,
                    title,
                    Some(EntityCategory::UnknownService),
                    coin.address.clone(),
                    Some(stx.txid.clone()),
                    coin.amount,
                    time,
                    hop,
                    Claim::Inference {
                        heuristic: "service.tx_shape".into(),
                        confidence: conf,
                        evidence: vec![shape.describe()],
                    },
                    &path,
                );
                continue;
            }

            // Ordinary transaction: allocate pro-rata to outputs.
            let total_in = stx.total_in().max(1) as u128;
            let change = detect_change(&stx);
            let peel = is_peel_step(&stx, change.as_ref());
            if peel {
                self.stats.peel_steps += 1;
            }
            let note = (shape != TxShape::Simple).then(|| shape.describe());
            let mut allocated = 0u64;
            for o in stx.outputs.iter().filter(|o| o.script_type.is_spendable()) {
                let share = (coin.amount as u128 * o.value as u128 / total_in) as u64;
                if share == 0 {
                    continue;
                }
                allocated += share;
                let role = match &change {
                    Some(c) if c.output == o.index && c.confidence.followable() => {
                        FlowRole::Change {
                            confidence: c.confidence,
                        }
                    }
                    _ => FlowRole::Payment,
                };
                self.flows.push(Flow {
                    txid: stx.txid.clone(),
                    hop,
                    time,
                    from: coin.address.clone(),
                    to: o.address.clone(),
                    amount: share,
                    role,
                    peel,
                    note: note.clone(),
                });
                push(
                    &mut pending,
                    Coin {
                        txid: stx.txid.clone(),
                        vout: o.index,
                        address: o.address.clone(),
                        amount: share,
                        hop,
                        time,
                        path: path.clone(),
                    },
                );
            }
            self.stats.fees += coin.amount - allocated;
        }
    }

    async fn backward(
        &mut self,
        seeds: Vec<Source>,
        min_amount: u64,
        progress: &mut (dyn FnMut(Progress) + Send),
    ) {
        let mut pending: HashMap<String, Source> = HashMap::new();
        let push = |pending: &mut HashMap<String, Source>, s: Source| {
            pending
                .entry(s.txid.clone())
                .and_modify(|e| {
                    e.amount += s.amount;
                    e.hop = e.hop.min(s.hop);
                })
                .or_insert(s);
        };
        for s in seeds {
            push(&mut pending, s);
        }

        while let Some(src) = pop_largest(&mut pending, |s| s.amount) {
            progress(Progress {
                txs_examined: self.stats.txs_examined,
                pending: pending.len(),
                leads: self.leads.len(),
            });
            if src.amount < min_amount {
                self.stats.below_threshold += src.amount;
                continue;
            }
            if src.hop > self.config.max_hops {
                self.unexplored(
                    StopReason::HopLimit,
                    src.receiver.clone(),
                    &src.txid,
                    src.amount,
                    src.hop,
                    &src.path,
                );
                continue;
            }
            if !self.budget_left(&src.txid) {
                self.unexplored(
                    StopReason::Budget,
                    src.receiver.clone(),
                    &src.txid,
                    src.amount,
                    src.hop,
                    &src.path,
                );
                continue;
            }
            let tx = match self.tx(&src.txid).await {
                Ok(t) => t,
                Err(e) => {
                    self.unexplored(
                        StopReason::Error(e.to_string()),
                        src.receiver.clone(),
                        &src.txid,
                        src.amount,
                        src.hop,
                        &src.path,
                    );
                    continue;
                }
            };
            let time = block_time(&tx);
            let shape = classify(&tx);
            match &shape {
                TxShape::Coinbase => {
                    let height = tx
                        .block
                        .as_ref()
                        .map(|b| b.height.to_string())
                        .unwrap_or_default();
                    self.add_lead(
                        LeadKind::Mined,
                        format!("Newly mined coins (block {height})"),
                        Some(EntityCategory::MiningPool),
                        src.receiver.clone(),
                        Some(tx.txid.clone()),
                        src.amount,
                        time,
                        src.hop,
                        Claim::Fact,
                        &src.path,
                    );
                    continue;
                }
                TxShape::CoinJoin(info) => {
                    self.add_lead(
                        LeadKind::Endpoint,
                        format!("Came out of {}", info.kind.label()),
                        Some(EntityCategory::Mixer),
                        None,
                        Some(tx.txid.clone()),
                        src.amount,
                        time,
                        src.hop,
                        Claim::Inference {
                            heuristic: "coinjoin".into(),
                            confidence: info.confidence,
                            evidence: info.evidence.clone(),
                        },
                        &src.path,
                    );
                    continue;
                }
                _ => {}
            }
            if let Some(conf) = shape.service_confidence()
                && conf >= self.config.stop_confidence
            {
                // Funds left a service wallet (withdrawal batch or swept hot wallet).
                let sender = tx.input_addresses().first().map(|a| (*a).clone());
                self.add_lead(
                    LeadKind::Endpoint,
                    format!("Likely service: funds sent in a {}", shape.describe()),
                    Some(EntityCategory::UnknownService),
                    sender,
                    Some(tx.txid.clone()),
                    src.amount,
                    time,
                    src.hop,
                    Claim::Inference {
                        heuristic: "service.tx_shape".into(),
                        confidence: conf,
                        evidence: vec![shape.describe()],
                    },
                    &src.path,
                );
                continue;
            }

            // Allocate pro-rata to inputs; remainder goes to the largest input so the
            // amounts add up exactly.
            let total_in = tx.total_in().max(1) as u128;
            let mut shares: Vec<u64> = tx
                .inputs
                .iter()
                .map(|i| (src.amount as u128 * i.value as u128 / total_in) as u64)
                .collect();
            let remainder = src.amount - shares.iter().sum::<u64>();
            if let Some(max_i) = (0..tx.inputs.len()).max_by_key(|&i| tx.inputs[i].value) {
                shares[max_i] += remainder;
            }
            let note = (shape != TxShape::Simple).then(|| shape.describe());
            for (input, share) in tx.inputs.iter().zip(shares) {
                if share == 0 {
                    continue;
                }
                let Some(prev_txid) = input.prev_txid.clone() else {
                    continue;
                };
                self.flows.push(Flow {
                    txid: tx.txid.clone(),
                    hop: src.hop,
                    time,
                    from: input.address.clone(),
                    to: src.receiver.clone(),
                    amount: share,
                    role: FlowRole::Source,
                    peel: false,
                    note: note.clone(),
                });
                let mut path = src.path.clone();
                path.push(prev_txid.clone());
                if let Some(addr) = input.address.clone()
                    && let Some((title, category, claim)) = self.endpoint_for(&addr).await
                {
                    self.add_lead(
                        LeadKind::Endpoint,
                        title,
                        Some(category),
                        Some(addr),
                        Some(prev_txid),
                        share,
                        time,
                        src.hop + 1,
                        claim,
                        &path,
                    );
                    continue;
                }
                push(
                    &mut pending,
                    Source {
                        txid: prev_txid,
                        amount: share,
                        hop: src.hop + 1,
                        receiver: input.address.clone(),
                        path,
                    },
                );
            }
        }
    }
}

/// Bitcoin addresses only; used by callers that build seeds from user input.
pub fn parse_btc_address(s: &str) -> Result<Address, ct_core::AddressError> {
    Address::parse(Chain::Bitcoin, s)
}
