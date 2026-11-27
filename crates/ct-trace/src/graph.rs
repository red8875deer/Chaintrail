//! A trace as a graph, for the interactive view.
//!
//! Nodes are addresses (plus transaction nodes for endpoints that have no single address,
//! such as a CoinJoin). Edges are traced value moving between them in one transaction.
//! Every node and edge refers back to the report, so the UI never shows anything the
//! report does not contain.

use std::collections::{HashMap, HashSet};

use ct_core::{Claim, Confidence, EntityCategory};
use serde::{Deserialize, Serialize};

use crate::report::{Direction, FlowRole, Lead, LeadKind, TraceReport};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Address,
    /// A transaction shown as a node (e.g. a CoinJoin the funds entered).
    Transaction,
    /// An input or output without an address (e.g. pay-to-public-key).
    NoAddress,
}

/// Lead information attached to a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLead {
    /// Index into `TraceReport::leads` (the main lead here: an endpoint, else the largest).
    pub index: usize,
    pub kind: LeadKind,
    pub title: String,
    /// Total of every lead at this node.
    pub amount: u64,
    pub confidence: Confidence,
    /// Other leads that end at the same node (the same address reached more than once).
    pub others: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: NodeKind,
    pub address: Option<String>,
    pub txid: Option<String>,
    /// Short display text: entity name, lead title or shortened address.
    pub label: String,
    /// Where the trace started.
    pub seed: bool,
    /// Fewest hops from the seed.
    pub hop: u32,
    /// Known owner, from a lead or a label passed through.
    pub entity: Option<String>,
    pub category: Option<EntityCategory>,
    /// How sure we are about `entity`/`category`.
    pub confidence: Option<Confidence>,
    pub lead: Option<NodeLead>,
    /// Traced value into / out of this node, base units.
    pub amount_in: u64,
    pub amount_out: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub txid: String,
    pub hop: u32,
    pub time: Option<u64>,
    /// Traced amount, base units.
    pub amount: u64,
    pub role: FlowRole,
    pub peel: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphView {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Shorten an address for display: `bc1qq2eu…6cfsegq`.
pub fn short_address(a: &str) -> String {
    if a.chars().count() <= 16 {
        return a.to_string();
    }
    let head: String = a.chars().take(8).collect();
    let tail: String = a
        .chars()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

struct Builder {
    nodes: Vec<GraphNode>,
    index: HashMap<String, usize>,
    edges: Vec<GraphEdge>,
    edge_index: HashMap<String, usize>,
}

impl Builder {
    fn node(&mut self, id: &str, kind: NodeKind, hop: u32) -> &mut GraphNode {
        let i = match self.index.get(id) {
            Some(&i) => i,
            None => {
                let (address, txid, label) = match kind {
                    NodeKind::Address => (Some(id.to_string()), None, short_address(id)),
                    NodeKind::Transaction => {
                        let t = id.trim_start_matches("tx:").to_string();
                        let label = format!("tx {}", short_address(&t));
                        (None, Some(t), label)
                    }
                    NodeKind::NoAddress => (None, None, "no address".to_string()),
                };
                self.nodes.push(GraphNode {
                    id: id.to_string(),
                    kind,
                    address,
                    txid,
                    label,
                    seed: false,
                    hop,
                    entity: None,
                    category: None,
                    confidence: None,
                    lead: None,
                    amount_in: 0,
                    amount_out: 0,
                });
                self.index.insert(id.to_string(), self.nodes.len() - 1);
                self.nodes.len() - 1
            }
        };
        let n = &mut self.nodes[i];
        n.hop = n.hop.min(hop);
        n
    }

    #[allow(clippy::too_many_arguments)]
    fn edge(
        &mut self,
        source: &str,
        target: &str,
        txid: &str,
        hop: u32,
        time: Option<u64>,
        amount: u64,
        role: FlowRole,
        peel: bool,
        note: Option<String>,
    ) {
        let id = format!("{txid}|{source}|{target}");
        if let Some(&i) = self.edge_index.get(&id) {
            self.edges[i].amount += amount;
        } else {
            self.edge_index.insert(id.clone(), self.edges.len());
            self.edges.push(GraphEdge {
                id,
                source: source.to_string(),
                target: target.to_string(),
                txid: txid.to_string(),
                hop,
                time,
                amount,
                role,
                peel,
                note,
            });
        }
        // Funds an address sends back to itself are not new funds in or out; counting them
        // would inflate the node's total beyond the traced amount.
        if source != target {
            self.nodes[self.index[source]].amount_out += amount;
            self.nodes[self.index[target]].amount_in += amount;
        }
    }
}

/// Node id: the address, or for outputs without one the coin itself (`noaddr:txid:vout`),
/// so the transfer that created it and the one that spent it meet at the same node.
fn node_id(
    address: Option<&ct_core::Address>,
    coin: Option<&str>,
    txid: &str,
    side: &str,
) -> (String, NodeKind) {
    match (address, coin) {
        (Some(a), _) => (a.to_string(), NodeKind::Address),
        // A coin reference without ":vout" is a whole transaction (CoinJoin, payout).
        (None, Some(c)) if !c.contains(':') => (format!("tx:{c}"), NodeKind::Transaction),
        (None, Some(c)) => (format!("noaddr:{c}"), NodeKind::NoAddress),
        (None, None) => (format!("noaddr:{txid}:{side}"), NodeKind::NoAddress),
    }
}

/// Build the graph view of a trace report.
/// Two leads end at the same node: keep both. The main one is an endpoint if there is one,
/// else the larger; the node's amount is their total.
fn merge_leads(a: NodeLead, b: NodeLead, leads: &[Lead]) -> NodeLead {
    let named = |l: &NodeLead| matches!(leads[l.index].kind, LeadKind::Endpoint | LeadKind::Mined);
    let (mut main, other) =
        if named(&b) && !named(&a) || (named(&b) == named(&a) && b.amount > a.amount) {
            (b, a)
        } else {
            (a, b)
        };
    main.amount += other.amount;
    main.others.push(other.index);
    main.others.extend(other.others);
    main.others.sort_unstable();
    main
}

pub fn build(report: &TraceReport) -> GraphView {
    let mut b = Builder {
        nodes: Vec::new(),
        index: HashMap::new(),
        edges: Vec::new(),
        edge_index: HashMap::new(),
    };
    let forward = report.direction == Direction::Forward;

    for f in &report.flows {
        // Forward: `from` sits one hop before the flow. Backward: the input (`from`) is one
        // hop further from the seed than the receiver.
        let (from_hop, to_hop) = if forward {
            (f.hop.saturating_sub(1), f.hop)
        } else {
            (f.hop + 1, f.hop)
        };
        let (from_id, from_kind) = node_id(f.from.as_ref(), f.from_coin.as_deref(), &f.txid, "in");
        let (to_id, to_kind) = node_id(f.to.as_ref(), f.to_coin.as_deref(), &f.txid, "out");
        b.node(&from_id, from_kind, from_hop);
        b.node(&to_id, to_kind, to_hop);
        b.edge(
            &from_id,
            &to_id,
            &f.txid,
            f.hop,
            f.time,
            f.amount,
            f.role.clone(),
            f.peel,
            f.note.clone(),
        );
        // Seeds: the starting side of the first hop.
        if forward && f.hop == 1 {
            b.node(&from_id, NodeKind::Address, 0).seed = true;
        }
        if !forward && f.hop == 0 {
            b.node(&to_id, NodeKind::Address, 0).seed = true;
        }
    }

    for (i, lead) in report.leads.iter().enumerate() {
        let info = NodeLead {
            index: i,
            kind: lead.kind.clone(),
            title: lead.title.clone(),
            amount: lead.amount,
            confidence: lead.confidence,
            others: Vec::new(),
        };
        let id = match (&lead.address, &lead.txid) {
            (Some(a), _) => {
                let id = a.to_string();
                b.node(&id, NodeKind::Address, lead.hops);
                id
            }
            // One coin without an address: the same node as the flows that reach it.
            (None, Some(t)) if lead.vout.is_some() => {
                let id = format!("noaddr:{t}:{}", lead.vout.unwrap_or_default());
                b.node(&id, NodeKind::NoAddress, lead.hops);
                id
            }
            (None, Some(t)) => {
                let id = format!("tx:{t}");
                b.node(&id, NodeKind::Transaction, lead.hops);
                id
            }
            (None, None) => continue,
        };
        // Connect leads that are not already reached by a flow (e.g. a CoinJoin entered
        // from `via`).
        if let Some(via) = &lead.via {
            let via_id = via.to_string();
            if via_id != id && !b.index.contains_key(&via_id) {
                b.node(&via_id, NodeKind::Address, lead.hops.saturating_sub(1));
            }
            let connected = b.edges.iter().any(|e| {
                (e.source == via_id && e.target == id) || (e.source == id && e.target == via_id)
            });
            if via_id != id && !connected {
                let txid = lead.txid.clone().unwrap_or_default();
                let (s, t) = if forward {
                    (&via_id, &id)
                } else {
                    (&id, &via_id)
                };
                let role = if forward {
                    FlowRole::Payment
                } else {
                    FlowRole::Source
                };
                b.edge(
                    s,
                    t,
                    &txid,
                    lead.hops,
                    lead.first_time,
                    lead.amount,
                    role,
                    false,
                    None,
                );
            }
        }
        if lead.hops == 0 && matches!(lead.kind, LeadKind::AtRest) {
            b.node(&id, NodeKind::Address, 0).seed = true;
        }
        let n = b.node(&id, NodeKind::Address, lead.hops);
        if matches!(lead.kind, LeadKind::Endpoint | LeadKind::Mined) {
            n.category = lead.category;
            n.confidence = Some(lead.confidence);
            n.entity = Some(lead.title.clone());
            n.label = lead
                .title
                .split(':')
                .next()
                .unwrap_or(&lead.title)
                .trim()
                .to_string();
        }
        n.lead = Some(match n.lead.take() {
            None => info,
            Some(prev) => merge_leads(prev, info, &report.leads),
        });
    }

    // A trace of several outputs at once (e.g. a whole transaction): draw the transaction
    // as the one start point, paying each traced output its exact amount. Otherwise outputs
    // that are themselves an endpoint or at rest would float unconnected.
    let seed_node = |o: &crate::report::SeedOutput| match &o.address {
        Some(a) => (a.to_string(), NodeKind::Address),
        None => (format!("noaddr:{}:{}", o.txid, o.vout), NodeKind::NoAddress),
    };
    let seed_nodes: HashSet<String> = report.seed_outputs.iter().map(|o| seed_node(o).0).collect();
    if forward && seed_nodes.len() > 1 {
        let txid = &report.seed_outputs[0].txid;
        let root = format!("tx:{txid}");
        b.node(&root, NodeKind::Transaction, 0).seed = true;
        for n in &mut b.nodes {
            if n.id != root {
                n.seed = false;
            }
        }
        for o in &report.seed_outputs {
            if o.amount < report.min_amount {
                continue; // not followed (counted as below the threshold)
            }
            let (id, kind) = seed_node(o);
            b.node(&id, kind, 0).seed = false;
            b.edge(
                &root,
                &id,
                txid,
                0,
                o.time,
                o.amount,
                FlowRole::Payment,
                false,
                None,
            );
        }
    }

    // Every graph has a start: e.g. a backward trace of a coinbase or a CoinJoin, where no
    // flow leaves the start, marks the traced output or transaction itself.
    if !b.nodes.iter().any(|n| n.seed) {
        let mut ids: Vec<String> = report.seed_outputs.iter().map(|o| seed_node(o).0).collect();
        if let Some(o) = report.seed_outputs.first() {
            ids.push(format!("tx:{}", o.txid));
        }
        if let Some(i) = ids.iter().find_map(|id| b.index.get(id).copied()) {
            b.nodes[i].seed = true;
        }
    }

    for seen in &report.labels_seen {
        let id = seen.address.to_string();
        if let Some(&i) = b.index.get(&id) {
            let n = &mut b.nodes[i];
            if n.entity.is_none() {
                n.entity = Some(seen.entity.clone());
                n.category = Some(seen.category);
                n.confidence = Some(seen.claim.confidence());
                n.label = seen.entity.clone();
            }
        }
    }

    // Endpoint nodes keep the claim's confidence; facts stay certain.
    for n in &mut b.nodes {
        if n.lead
            .as_ref()
            .is_some_and(|l| matches!(l.kind, LeadKind::AtRest))
            && n.confidence.is_none()
        {
            n.confidence = Some(Claim::Fact.confidence());
        }
    }

    GraphView {
        nodes: b.nodes,
        edges: b.edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens() {
        assert_eq!(
            short_address("bc1qq2euq8pw950klpjcawuy4uj39ym43hs6cfsegq"),
            "bc1qq2eu…cfsegq"
        );
        assert_eq!(short_address("short"), "short");
    }
}
