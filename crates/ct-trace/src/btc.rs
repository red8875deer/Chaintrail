//! Bitcoin tracing.
//!
//! Value is followed largest-first, so a limited budget is spent on the biggest flows.
//! Allocation is pro-rata (`docs/TRACING.md` §5): a transaction's traced share of its
//! inputs is passed to each output in proportion to the output's value. Fees and rounding
//! are recorded, so leads + fees + dust always add up to the seed amount.

use std::collections::HashMap;

use ct_core::{
    Address, AddressSummary, Chain, Claim, Confidence, EntityCategory, Label, OutputSpend, UtxoTx,
};
use ct_heuristics::btc::coinjoin::is_standard_denomination;
use ct_heuristics::btc::dormancy::SAMPLE_INPUTS;
use ct_heuristics::btc::{
    Dormancy, TxShape, classify, detect_change, dormancy, is_peel_step, service_from_summary,
};
use ct_providers::{BitcoinProvider, ProviderError};
use serde::{Deserialize, Serialize};

use crate::labels::LabelLookup;
use crate::report::{
    Direction, Flow, FlowRole, LabelSeen, Lead, LeadKind, Progress, SeedOutput, StopReason,
    TraceConfig, TraceReport, TraceStats,
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
        samples: HashMap::new(),
        leads: Vec::new(),
        lead_index: HashMap::new(),
        flows: Vec::new(),
        labels_seen: Vec::new(),
        stats: TraceStats::default(),
        via: None,
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
    let seed_list: Vec<SeedOutput> = seed_outputs
        .iter()
        .map(|(vout, address, amount)| SeedOutput {
            txid: seed_tx.txid.clone(),
            vout: *vout,
            address: address.clone(),
            amount: *amount,
            time: block_time(&seed_tx),
        })
        .collect();

    // A label on the start address is reported too (it never stops the trace there).
    for (_, address, _) in &seed_outputs {
        if let Some(a) = address
            && let Some(l) = labels.lookup(a)
        {
            t.saw_label(l);
        }
    }
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
            // A whole transaction is traced back as one receiver: the transaction itself.
            let receivers = match &seed {
                Seed::Tx { .. } => vec![Receiver {
                    address: None,
                    coin: Some(seed_tx.txid.clone()),
                    amount: seed_amount,
                }],
                _ => seed_outputs
                    .iter()
                    .map(|(vout, address, value)| Receiver {
                        address: address.clone(),
                        coin: Some(format!("{}:{vout}", seed_tx.txid)),
                        amount: *value,
                    })
                    .collect(),
            };
            let source = Source {
                txid: seed_tx.txid.clone(),
                hop: 0,
                receivers,
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
        seed_outputs: seed_list,
        allocation: "pro_rata".into(),
        min_amount,
        config,
        leads,
        flows: t.flows,
        labels_seen: t.labels_seen,
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

/// Traced funds received by one output, whose origin is followed backward.
#[derive(Debug, Clone)]
struct Receiver {
    address: Option<Address>,
    /// `txid:vout` of that output, or a bare txid when the receiver is a whole transaction
    /// (a backward trace of every input of a transaction).
    coin: Option<String>,
    amount: u64,
}

impl Receiver {
    fn vout(&self) -> Option<u32> {
        self.coin.as_ref()?.rsplit_once(':')?.1.parse().ok()
    }
}

/// A transaction whose inputs funded traced outputs (backward). The outputs it funded are
/// kept apart, so each one is credited only with what it received.
#[derive(Debug, Clone)]
struct Source {
    txid: String,
    hop: u32,
    receivers: Vec<Receiver>,
    path: Vec<String>,
}

impl Source {
    fn amount(&self) -> u64 {
        self.receivers.iter().map(|r| r.amount).sum()
    }
}

struct Tracer<'a> {
    provider: &'a dyn BitcoinProvider,
    labels: &'a dyn LabelLookup,
    config: TraceConfig,
    txs: HashMap<String, UtxoTx>,
    spends: HashMap<String, Vec<OutputSpend>>,
    summaries: HashMap<Address, Option<AddressSummary>>,
    /// Transactions fetched only to look at, outside the trace's budget (`sample_tx`).
    samples: HashMap<String, UtxoTx>,
    leads: Vec<Lead>,
    lead_index: HashMap<String, usize>,
    flows: Vec<Flow>,
    labels_seen: Vec<LabelSeen>,
    stats: TraceStats,
    /// Address currently being processed; stamped on leads as `via`.
    via: Option<Address>,
}

/// A service named through labels on addresses of the same wallet.
struct NamedService {
    label: Label,
    confidence: Confidence,
    evidence: Vec<String>,
}

fn label_title(label: &Label) -> String {
    if label.entity.is_empty() {
        label.category.as_str().to_string()
    } else {
        format!("{} ({})", label.entity, label.category.as_str())
    }
}

fn label_evidence(label: &Label) -> String {
    match &label.claim {
        Claim::Attribution {
            source, url, as_of, ..
        } => format!(
            "labelled {} by {source}{}{}",
            label.entity,
            as_of
                .as_deref()
                .map(|d| format!(" (as of {d})"))
                .unwrap_or_default(),
            url.as_deref().map(|u| format!(": {u}")).unwrap_or_default()
        ),
        _ => format!("labelled {}", label.entity),
    }
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

    /// A transaction fetched only to look at (e.g. the age of a consolidation's inputs). It
    /// does not use the trace's transaction budget, which is for following funds; it has
    /// its own allowance of the same size.
    async fn sample_tx(&mut self, txid: &str) -> Option<UtxoTx> {
        if let Some(t) = self.txs.get(txid).or_else(|| self.samples.get(txid)) {
            return Some(t.clone());
        }
        if self.samples.len() as u32 >= self.config.max_txs {
            return None;
        }
        let t = self.provider.tx(txid).await.ok()?;
        self.samples.insert(txid.to_string(), t.clone());
        Some(t)
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
    /// `endpoint_for` from what is already known (labels, address checks made earlier in
    /// this trace), without network requests.
    fn known_endpoint(&self, address: &Address) -> Option<(String, EntityCategory, Claim)> {
        if let Some(label) = self.labels.lookup(address)
            && self.stops_at(&label)
        {
            return Some((label_title(&label), label.category, label.claim));
        }
        let hint = self
            .summaries
            .get(address)?
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

    async fn endpoint_for(&mut self, address: &Address) -> Option<(String, EntityCategory, Claim)> {
        if let Some(label) = self.labels.lookup(address) {
            if self.stops_at(&label) {
                return Some((label_title(&label), label.category, label.claim));
            }
            self.saw_label(label);
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

    /// For a consolidation, sample the largest inputs' ages. Mostly long-dormant inputs mean
    /// an owner gathering old coins, not a service sweeping fresh deposits.
    async fn consolidation_dormancy(&mut self, tx: &UtxoTx) -> Option<Dormancy> {
        let spend_time = tx.block.as_ref()?.time;
        let mut inputs: Vec<_> = tx.inputs.iter().filter(|i| i.prev_txid.is_some()).collect();
        inputs.sort_by_key(|i| std::cmp::Reverse(i.value));
        let mut prevs: Vec<String> = Vec::new();
        for i in inputs {
            let p = i.prev_txid.clone().expect("filtered");
            if !prevs.contains(&p) {
                prevs.push(p);
            }
            if prevs.len() == SAMPLE_INPUTS {
                break;
            }
        }
        let mut created = Vec::new();
        for p in prevs {
            if let Some(prev) = self.sample_tx(&p).await
                && let Some(b) = prev.block
            {
                created.push(b.time);
            }
        }
        dormancy(spend_time, &created)
    }

    /// Whether a consolidation merges coins fresh out of CoinJoins: most inputs use standard
    /// CoinJoin amounts and most sampled inputs were created by CoinJoin transactions.
    /// Returns (standard inputs, sampled inputs from CoinJoins, inputs sampled).
    async fn postmix_evidence(&mut self, tx: &UtxoTx) -> Option<(usize, usize, usize)> {
        let standard = tx
            .inputs
            .iter()
            .filter(|i| is_standard_denomination(i.value))
            .count();
        if tx.inputs.len() < 5 || standard * 10 < tx.inputs.len() * 8 {
            return None;
        }
        let mut inputs: Vec<_> = tx.inputs.iter().filter(|i| i.prev_txid.is_some()).collect();
        inputs.sort_by_key(|i| std::cmp::Reverse(i.value));
        let mut prevs: Vec<String> = Vec::new();
        for i in inputs {
            let p = i.prev_txid.clone().expect("filtered");
            if !prevs.contains(&p) {
                prevs.push(p);
            }
            if prevs.len() == SAMPLE_INPUTS {
                break;
            }
        }
        let mut sampled = 0;
        let mut from_coinjoins = 0;
        for p in prevs {
            if let Some(prev) = self.sample_tx(&p).await {
                sampled += 1;
                if matches!(classify(&prev), TxShape::CoinJoin(_)) {
                    from_coinjoins += 1;
                }
            }
        }
        (sampled >= 2 && from_coinjoins * 10 >= sampled * 6).then_some((
            standard,
            from_coinjoins,
            sampled,
        ))
    }

    /// The service confidence of a transaction shape, unless it is a consolidation of
    /// long-dormant coins. Returns the confidence (if it still counts as a service) and a
    /// note explaining a downgrade.
    async fn service_shape(
        &mut self,
        tx: &UtxoTx,
        shape: &TxShape,
    ) -> (Option<Confidence>, Option<String>) {
        let Some(conf) = shape
            .service_confidence()
            .filter(|c| *c >= self.config.stop_confidence)
        else {
            return (None, None);
        };
        // Coins straight out of CoinJoins (standard amounts, created by CoinJoin rounds)
        // swept together: one user merging their mixed coins, where the trail continues,
        // not a service sweeping deposits. Round amounts alone are not enough: ordinary
        // deposits are often round too.
        if matches!(shape, TxShape::Consolidation { .. })
            && let Some((standard, from_coinjoins, sampled)) = self.postmix_evidence(tx).await
        {
            return (
                None,
                Some(format!(
                    "{}; {standard} of {} inputs are standard CoinJoin amounts and {from_coinjoins} of {sampled} sampled inputs came out of CoinJoins: treated as one user merging mixed coins, not a service",
                    shape.describe(),
                    tx.inputs.len()
                )),
            );
        }
        if matches!(shape, TxShape::Consolidation { .. })
            && let Some(d) = self.consolidation_dormancy(tx).await
            && d.mostly_dormant()
        {
            return (
                None,
                Some(format!(
                    "{}; {}: treated as the owner consolidating old coins, not a service sweep",
                    shape.describe(),
                    d.describe()
                )),
            );
        }
        (Some(conf), None)
    }

    /// Whether a label is strong enough, and of an endpoint category, to stop the trace.
    fn stops_at(&self, label: &Label) -> bool {
        label.category.is_endpoint() && label.claim.confidence() >= self.config.stop_confidence
    }

    fn saw_label(&mut self, label: Label) {
        if !self.labels_seen.iter().any(|l| l.address == label.address) {
            self.labels_seen.push(LabelSeen {
                address: label.address,
                entity: label.entity,
                category: label.category,
                claim: label.claim,
            });
        }
    }

    /// Name the wallet behind a service-shaped transaction from labels on its inputs (all
    /// inputs of a non-CoinJoin transaction belong to one wallet) or, for consolidations,
    /// on the address that received the swept funds (weaker: capped at Medium).
    fn name_service(&self, tx: &UtxoTx, check_outputs: bool) -> Option<NamedService> {
        let best_input = tx
            .input_addresses()
            .into_iter()
            .filter_map(|a| self.labels.lookup(a))
            .filter(|l| self.stops_at(l))
            .max_by_key(|l| l.claim.confidence());
        if let Some(label) = best_input {
            let confidence = label.claim.confidence();
            let evidence = vec![
                format!(
                    "{} was spent together with the traced funds in this transaction",
                    label.address
                ),
                label_evidence(&label),
                "inputs spent together (outside CoinJoins) belong to one wallet".into(),
            ];
            return Some(NamedService {
                label,
                confidence,
                evidence,
            });
        }
        if !check_outputs {
            return None;
        }
        let label = tx
            .outputs
            .iter()
            .filter_map(|o| o.address.as_ref())
            .filter_map(|a| self.labels.lookup(a))
            .filter(|l| self.stops_at(l))
            .max_by_key(|l| l.claim.confidence())?;
        let confidence = label.claim.confidence().min(Confidence::Medium);
        let evidence = vec![
            format!("the swept funds went to {}", label.address),
            label_evidence(&label),
        ];
        Some(NamedService {
            label,
            confidence,
            evidence,
        })
    }

    /// For a CoinJoin, a labelled mixer address among its inputs or outputs (e.g. a
    /// coordinator fee address) names the mixer.
    fn name_mixer(&self, tx: &UtxoTx) -> Option<Label> {
        tx.inputs
            .iter()
            .filter_map(|i| i.address.as_ref())
            .chain(tx.outputs.iter().filter_map(|o| o.address.as_ref()))
            .filter_map(|a| self.labels.lookup(a))
            .find(|l| l.category == EntityCategory::Mixer)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_lead(
        &mut self,
        kind: LeadKind,
        title: String,
        category: Option<EntityCategory>,
        address: Option<Address>,
        txid: Option<String>,
        vout: Option<u32>,
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
        // Branches not followed stay one lead per coin, so each can be picked up exactly.
        let coin = match (&kind, &txid, vout) {
            (LeadKind::Unexplored { .. }, Some(t), Some(v)) => format!("{t}:{v}"),
            _ => String::new(),
        };
        let key = format!("{tag}|{place}|{title}|{coin}");
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
            via: self.via.clone(),
            txid,
            vout,
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

    #[allow(clippy::too_many_arguments)]
    fn unexplored(
        &mut self,
        stop: StopReason,
        address: Option<Address>,
        txid: &str,
        vout: Option<u32>,
        amount: u64,
        time: Option<u64>,
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
            vout,
            amount,
            time,
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
        // Fewest hops seen for each coin, with that route: a coin reached again by a longer
        // route keeps its shortest one (hops and path always agree).
        let mut best: HashMap<String, (u32, Vec<String>)> = HashMap::new();
        let push = |pending: &mut HashMap<String, Coin>,
                    best: &mut HashMap<String, (u32, Vec<String>)>,
                    mut c: Coin| {
            let key = format!("{}:{}", c.txid, c.vout);
            match best.get(&key) {
                Some((h, p)) if *h <= c.hop => {
                    c.hop = *h;
                    c.path = p.clone();
                }
                _ => {
                    best.insert(key.clone(), (c.hop, c.path.clone()));
                }
            }
            match pending.get_mut(&key) {
                Some(e) => {
                    e.amount += c.amount;
                    if c.hop < e.hop {
                        e.hop = c.hop;
                        e.path = c.path;
                    }
                }
                None => {
                    pending.insert(key, c);
                }
            }
        };
        for c in seeds {
            push(&mut pending, &mut best, c);
        }

        while let Some(coin) = pop_largest(&mut pending, |c| c.amount) {
            self.via = coin.address.clone();
            progress(Progress {
                txs_examined: self.stats.txs_examined,
                pending: pending.len(),
                leads: self.leads.len(),
            });
            if coin.amount < min_amount {
                // A small amount reaching an address already known as an endpoint still
                // belongs to it (e.g. a test payment before the main deposit). Only what is
                // already known is used: no extra lookups for small amounts.
                if coin.hop > 0
                    && let Some(addr) = coin.address.clone()
                    && let Some((title, category, claim)) = self.known_endpoint(&addr)
                {
                    self.add_lead(
                        LeadKind::Endpoint,
                        title,
                        Some(category),
                        Some(addr),
                        Some(coin.txid.clone()),
                        None,
                        coin.amount,
                        coin.time,
                        coin.hop,
                        claim,
                        &coin.path,
                    );
                    continue;
                }
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
                    None,
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
                        Some(coin.vout),
                        coin.amount,
                        coin.time,
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
                    Some(coin.vout),
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
                    Some(coin.vout),
                    coin.amount,
                    coin.time,
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
                    Some(coin.vout),
                    coin.amount,
                    coin.time,
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
                        Some(coin.vout),
                        coin.amount,
                        coin.time,
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
                let mut evidence = info.evidence.clone();
                let title = match self.name_mixer(&stx) {
                    Some(l) => {
                        evidence.push(format!(
                            "{} in this transaction: {}",
                            l.address,
                            label_evidence(&l)
                        ));
                        format!("Likely mixed in {}: {}", l.entity, info.kind.label())
                    }
                    None => format!("Likely mixed in {}", info.kind.label()),
                };
                // Each coin entering the CoinJoin gets its own edge to the transaction.
                self.flows.push(Flow {
                    txid: stx.txid.clone(),
                    hop,
                    time,
                    from: coin.address.clone(),
                    to: None,
                    from_coin: Some(format!("{}:{}", coin.txid, coin.vout)),
                    to_coin: Some(stx.txid.clone()),
                    amount: coin.amount,
                    role: FlowRole::Payment,
                    peel: false,
                    note: Some(shape.describe()),
                });
                self.add_lead(
                    LeadKind::Endpoint,
                    title,
                    Some(EntityCategory::Mixer),
                    None,
                    Some(stx.txid.clone()),
                    None,
                    coin.amount,
                    time,
                    hop,
                    Claim::Inference {
                        heuristic: "coinjoin".into(),
                        confidence: info.confidence,
                        evidence,
                    },
                    &path,
                );
                continue;
            }
            // Not for the seed coin: the user chose to trace from there (a scammer's address
            // that sweeps many victims' payments is not a service).
            let (service, dormant_note) =
                at_start(coin.hop, self.service_shape(&stx, &shape).await, &shape);
            if let Some(conf) = service {
                let what = match &shape {
                    TxShape::Consolidation {
                        distinct_inputs, ..
                    } => format!(
                        "deposit swept together with {} other addresses",
                        distinct_inputs.saturating_sub(1)
                    ),
                    _ => format!("wallet paid out in a {}", shape.describe()),
                };
                let consolidation = matches!(shape, TxShape::Consolidation { .. });
                let (title, category, claim) = match self.name_service(&stx, consolidation) {
                    Some(n) => {
                        let mut evidence = vec![shape.describe()];
                        evidence.extend(n.evidence);
                        (
                            format!("Likely {}: {what}", label_title(&n.label)),
                            n.label.category,
                            Claim::Inference {
                                heuristic: "label.same_wallet".into(),
                                // Both the shape and the label must hold.
                                confidence: n.confidence.min(conf),
                                evidence,
                            },
                        )
                    }
                    None => (
                        format!("Likely service: {what}"),
                        EntityCategory::UnknownService,
                        Claim::Inference {
                            heuristic: "service.tx_shape".into(),
                            confidence: conf,
                            evidence: vec![shape.describe()],
                        },
                    ),
                };
                self.add_lead(
                    LeadKind::Endpoint,
                    title,
                    Some(category),
                    coin.address.clone(),
                    Some(stx.txid.clone()),
                    None,
                    coin.amount,
                    time,
                    hop,
                    claim,
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
            let note =
                dormant_note.or_else(|| (shape != TxShape::Simple).then(|| shape.describe()));
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
                    from_coin: Some(format!("{}:{}", coin.txid, coin.vout)),
                    to_coin: Some(format!("{}:{}", stx.txid, o.index)),
                    amount: share,
                    role,
                    peel,
                    note: note.clone(),
                });
                push(
                    &mut pending,
                    &mut best,
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

    /// One "not followed" lead per traced output of a source.
    fn unexplored_receivers(&mut self, stop: StopReason, src: &Source) {
        for r in &src.receivers {
            self.via = r.address.clone();
            self.unexplored(
                stop.clone(),
                r.address.clone(),
                &src.txid,
                r.vout(),
                r.amount,
                None,
                src.hop,
                &src.path,
            );
        }
    }

    /// Funds that came out of a transaction as a whole (a CoinJoin, a service payout): one
    /// edge from the transaction to each traced output it funded.
    fn flows_from_tx(
        &mut self,
        tx: &UtxoTx,
        src: &Source,
        time: Option<u64>,
        note: Option<String>,
    ) {
        for r in &src.receivers {
            if r.coin.as_deref() == Some(tx.txid.as_str()) {
                continue; // the receiver is this transaction itself
            }
            self.flows.push(Flow {
                txid: tx.txid.clone(),
                hop: src.hop,
                time,
                from: None,
                to: r.address.clone(),
                from_coin: Some(tx.txid.clone()),
                to_coin: r.coin.clone(),
                amount: r.amount,
                role: FlowRole::Source,
                peel: false,
                note: note.clone(),
            });
        }
    }

    async fn backward(
        &mut self,
        seeds: Vec<Source>,
        min_amount: u64,
        progress: &mut (dyn FnMut(Progress) + Send),
    ) {
        let mut pending: HashMap<String, Source> = HashMap::new();
        // Fewest hops seen for each transaction, with that route.
        let mut best: HashMap<String, (u32, Vec<String>)> = HashMap::new();
        let push = |pending: &mut HashMap<String, Source>,
                    best: &mut HashMap<String, (u32, Vec<String>)>,
                    mut s: Source| {
            match best.get(&s.txid) {
                Some((h, p)) if *h <= s.hop => {
                    s.hop = *h;
                    s.path = p.clone();
                }
                _ => {
                    best.insert(s.txid.clone(), (s.hop, s.path.clone()));
                }
            }
            match pending.get_mut(&s.txid) {
                Some(e) => {
                    for r in s.receivers {
                        match e
                            .receivers
                            .iter_mut()
                            .find(|x| x.coin == r.coin && x.address == r.address)
                        {
                            Some(x) => x.amount += r.amount,
                            None => e.receivers.push(r),
                        }
                    }
                    if s.hop < e.hop {
                        e.hop = s.hop;
                        e.path = s.path;
                    }
                }
                None => {
                    pending.insert(s.txid.clone(), s);
                }
            }
        };
        for s in seeds {
            push(&mut pending, &mut best, s);
        }

        while let Some(src) = pop_largest(&mut pending, |s| s.amount()) {
            self.via = src
                .receivers
                .iter()
                .max_by_key(|r| r.amount)
                .and_then(|r| r.address.clone());
            progress(Progress {
                txs_examined: self.stats.txs_examined,
                pending: pending.len(),
                leads: self.leads.len(),
            });
            let amount = src.amount();
            if amount < min_amount {
                self.stats.below_threshold += amount;
                continue;
            }
            // Same limit as forward: funds are shown up to `max_hops` transactions away.
            if src.hop >= self.config.max_hops {
                self.unexplored_receivers(StopReason::HopLimit, &src);
                continue;
            }
            if !self.budget_left(&src.txid) {
                self.unexplored_receivers(StopReason::Budget, &src);
                continue;
            }
            let tx = match self.tx(&src.txid).await {
                Ok(t) => t,
                Err(e) => {
                    self.unexplored_receivers(StopReason::Error(e.to_string()), &src);
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
                    for r in &src.receivers {
                        self.via = r.address.clone();
                        self.add_lead(
                            LeadKind::Mined,
                            format!("Newly mined coins (block {height})"),
                            None,
                            r.address.clone(),
                            Some(tx.txid.clone()),
                            r.vout(),
                            r.amount,
                            time,
                            src.hop,
                            Claim::Fact,
                            &src.path,
                        );
                    }
                    continue;
                }
                TxShape::CoinJoin(info) => {
                    let mut evidence = info.evidence.clone();
                    let title = match self.name_mixer(&tx) {
                        Some(l) => {
                            evidence.push(format!(
                                "{} in this transaction: {}",
                                l.address,
                                label_evidence(&l)
                            ));
                            format!("Likely came out of {}: {}", l.entity, info.kind.label())
                        }
                        None => format!("Likely came out of {}", info.kind.label()),
                    };
                    self.flows_from_tx(&tx, &src, time, Some(shape.describe()));
                    self.add_lead(
                        LeadKind::Endpoint,
                        title,
                        Some(EntityCategory::Mixer),
                        None,
                        Some(tx.txid.clone()),
                        None,
                        amount,
                        time,
                        src.hop,
                        Claim::Inference {
                            heuristic: "coinjoin".into(),
                            confidence: info.confidence,
                            evidence,
                        },
                        &src.path,
                    );
                    continue;
                }
                _ => {}
            }
            // Not for the seed transaction: the user chose to trace back from there.
            // A service payout is a stop even for the start transaction: that is where the
            // funds came from.
            let (service, dormant_note) = self.service_shape(&tx, &shape).await;
            if let Some(conf) = service {
                // Funds left a service wallet (withdrawal batch or swept hot wallet). The
                // whole transaction is the service's: drawn as the transaction, not as one
                // of its input addresses.
                let what = format!("funds sent in a {}", shape.describe());
                let (title, category, claim) = match self.name_service(&tx, false) {
                    Some(n) => {
                        let mut evidence = vec![shape.describe()];
                        evidence.extend(n.evidence);
                        (
                            format!("Likely {}: {what}", label_title(&n.label)),
                            n.label.category,
                            Claim::Inference {
                                heuristic: "label.same_wallet".into(),
                                confidence: n.confidence.min(conf),
                                evidence,
                            },
                        )
                    }
                    None => (
                        format!("Likely service: {what}"),
                        EntityCategory::UnknownService,
                        Claim::Inference {
                            heuristic: "service.tx_shape".into(),
                            confidence: conf,
                            evidence: vec![shape.describe()],
                        },
                    ),
                };
                self.flows_from_tx(&tx, &src, time, Some(shape.describe()));
                self.add_lead(
                    LeadKind::Endpoint,
                    title,
                    Some(category),
                    None,
                    Some(tx.txid.clone()),
                    None,
                    amount,
                    time,
                    src.hop,
                    claim,
                    &src.path,
                );
                continue;
            }

            // Allocate each traced output's amount pro-rata to the inputs (remainder to the
            // largest input, so the amounts add up exactly), one edge per input and output.
            let total_in = tx.total_in().max(1) as u128;
            let largest = (0..tx.inputs.len()).max_by_key(|&i| tx.inputs[i].value);
            let mut per_input = vec![0u64; tx.inputs.len()];
            let note =
                dormant_note.or_else(|| (shape != TxShape::Simple).then(|| shape.describe()));
            for r in &src.receivers {
                let mut shares: Vec<u64> = tx
                    .inputs
                    .iter()
                    .map(|i| (r.amount as u128 * i.value as u128 / total_in) as u64)
                    .collect();
                let remainder = r.amount - shares.iter().sum::<u64>();
                if let Some(m) = largest {
                    shares[m] += remainder;
                }
                for (k, input) in tx.inputs.iter().enumerate() {
                    let share = shares[k];
                    if share == 0 {
                        continue;
                    }
                    let Some(prev_txid) = &input.prev_txid else {
                        continue;
                    };
                    self.flows.push(Flow {
                        txid: tx.txid.clone(),
                        hop: src.hop,
                        time,
                        from: input.address.clone(),
                        to: r.address.clone(),
                        from_coin: Some(format!("{prev_txid}:{}", input.prev_vout)),
                        to_coin: r.coin.clone(),
                        amount: share,
                        role: FlowRole::Source,
                        peel: false,
                        note: note.clone(),
                    });
                    per_input[k] += share;
                }
            }
            for (k, input) in tx.inputs.iter().enumerate() {
                let share = per_input[k];
                if share == 0 {
                    continue;
                }
                let Some(prev_txid) = input.prev_txid.clone() else {
                    continue;
                };
                let mut path = src.path.clone();
                path.push(prev_txid.clone());
                self.via = input.address.clone();
                if let Some(addr) = input.address.clone()
                    && let Some((title, category, claim)) = self.endpoint_for(&addr).await
                {
                    self.add_lead(
                        LeadKind::Endpoint,
                        title,
                        Some(category),
                        Some(addr),
                        Some(prev_txid),
                        None,
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
                    &mut best,
                    Source {
                        txid: prev_txid.clone(),
                        hop: src.hop + 1,
                        receivers: vec![Receiver {
                            address: input.address.clone(),
                            coin: Some(format!("{prev_txid}:{}", input.prev_vout)),
                            amount: share,
                        }],
                        path,
                    },
                );
            }
        }
    }
}

/// Forward, the start address being swept with many others does not stop the trace: it may
/// be a service sweeping deposits (then the start address is a deposit address there), or
/// its owner gathering funds, e.g. a scammer sweeping victims' payments. The note says so.
fn at_start(
    hop: u32,
    verdict: (Option<Confidence>, Option<String>),
    shape: &TxShape,
) -> (Option<Confidence>, Option<String>) {
    match verdict {
        (Some(_), note) if hop == 0 => (
            None,
            note.or_else(|| {
                Some(format!(
                    "{}: the start address was swept with others, either by a service (then it is a deposit address there) or by its owner gathering funds; followed because the trace starts here",
                    shape.describe()
                ))
            }),
        ),
        v => v,
    }
}

/// Bitcoin addresses only; used by callers that build seeds from user input.
pub fn parse_btc_address(s: &str) -> Result<Address, ct_core::AddressError> {
    Address::parse(Chain::Bitcoin, s)
}
