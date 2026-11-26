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
            let receiver_coin = seed_outputs
                .first()
                .map(|(vout, _, _)| format!("{}:{vout}", seed_tx.txid));
            let source = Source {
                txid: seed_tx.txid.clone(),
                amount: seed_amount,
                hop: 0,
                receiver,
                receiver_coin,
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

/// A traced amount to explain by the inputs of `txid` (backward).
#[derive(Debug, Clone)]
struct Source {
    txid: String,
    amount: u64,
    hop: u32,
    /// Address that received the traced funds in `txid`.
    receiver: Option<Address>,
    /// The output (`txid:vout`) that received them.
    receiver_coin: Option<String>,
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
            if !self.budget_left(&p) {
                break;
            }
            if let Ok(prev) = self.tx(&p).await
                && let Some(b) = prev.block
            {
                created.push(b.time);
            }
        }
        dormancy(spend_time, &created)
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
            via: self.via.clone(),
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
            self.via = coin.address.clone();
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
                let mut evidence = info.evidence.clone();
                let title = match self.name_mixer(&stx) {
                    Some(l) => {
                        evidence.push(format!(
                            "{} in this transaction: {}",
                            l.address,
                            label_evidence(&l)
                        ));
                        format!("Mixed in {}: {}", l.entity, info.kind.label())
                    }
                    None => format!("Mixed in {}", info.kind.label()),
                };
                self.add_lead(
                    LeadKind::Endpoint,
                    title,
                    Some(EntityCategory::Mixer),
                    None,
                    Some(stx.txid.clone()),
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
            let (service, dormant_note) = self.service_shape(&stx, &shape).await;
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
                            format!("{}: {what}", label_title(&n.label)),
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
            self.via = src.receiver.clone();
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
                    let mut evidence = info.evidence.clone();
                    let title = match self.name_mixer(&tx) {
                        Some(l) => {
                            evidence.push(format!(
                                "{} in this transaction: {}",
                                l.address,
                                label_evidence(&l)
                            ));
                            format!("Came out of {}: {}", l.entity, info.kind.label())
                        }
                        None => format!("Came out of {}", info.kind.label()),
                    };
                    self.add_lead(
                        LeadKind::Endpoint,
                        title,
                        Some(EntityCategory::Mixer),
                        None,
                        Some(tx.txid.clone()),
                        src.amount,
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
            let (service, dormant_note) = self.service_shape(&tx, &shape).await;
            if let Some(conf) = service {
                // Funds left a service wallet (withdrawal batch or swept hot wallet).
                let what = format!("funds sent in a {}", shape.describe());
                let (title, category, sender, claim) = match self.name_service(&tx, false) {
                    Some(n) => {
                        let mut evidence = vec![shape.describe()];
                        evidence.extend(n.evidence);
                        (
                            format!("{}: {what}", label_title(&n.label)),
                            n.label.category,
                            Some(n.label.address.clone()),
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
                        tx.input_addresses().first().map(|a| (*a).clone()),
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
                    sender,
                    Some(tx.txid.clone()),
                    src.amount,
                    time,
                    src.hop,
                    claim,
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
            let note =
                dormant_note.or_else(|| (shape != TxShape::Simple).then(|| shape.describe()));
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
                    from_coin: Some(format!("{prev_txid}:{}", input.prev_vout)),
                    to_coin: src.receiver_coin.clone(),
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
                        receiver_coin: Some(format!("{prev_txid}:{}", input.prev_vout)),
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
