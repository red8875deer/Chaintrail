//! Address clustering with recorded evidence (`docs/TRACING.md` §3.1).
//!
//! Union-find over addresses. Every merge keeps the transaction and heuristic that caused
//! it, so a wrong merge can be explained and undone. CoinJoins and coinbases are never used
//! for common-input merges; change outputs are merged only at High confidence.

use std::collections::HashMap;

use ct_core::{Address, Confidence, UtxoTx};
use serde::{Deserialize, Serialize};

use super::change::ChangeGuess;
use super::coinjoin::detect_coinjoin;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeEvidence {
    pub a: Address,
    pub b: Address,
    pub txid: String,
    pub heuristic: String,
}

#[derive(Debug, Default, Clone)]
pub struct Clusters {
    index: HashMap<Address, usize>,
    addresses: Vec<Address>,
    parent: Vec<usize>,
    evidence: Vec<MergeEvidence>,
}

impl Clusters {
    pub fn new() -> Self {
        Self::default()
    }

    fn id(&mut self, a: &Address) -> usize {
        if let Some(&i) = self.index.get(a) {
            return i;
        }
        let i = self.addresses.len();
        self.index.insert(a.clone(), i);
        self.addresses.push(a.clone());
        self.parent.push(i);
        i
    }

    fn root(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, a: &Address, b: &Address, txid: &str, heuristic: &str) -> bool {
        let (ia, ib) = (self.id(a), self.id(b));
        let (ra, rb) = (self.root(ia), self.root(ib));
        if ra == rb {
            return false;
        }
        self.parent[rb] = ra;
        self.evidence.push(MergeEvidence {
            a: a.clone(),
            b: b.clone(),
            txid: txid.to_string(),
            heuristic: heuristic.to_string(),
        });
        true
    }

    /// Apply common-input ownership to `tx`. Returns the number of merges.
    pub fn add_common_inputs(&mut self, tx: &UtxoTx) -> usize {
        if tx.is_coinbase() || detect_coinjoin(tx).is_some() {
            return 0;
        }
        let addrs = tx.input_addresses();
        let Some(first) = addrs.first().copied().cloned() else {
            return 0;
        };
        addrs
            .iter()
            .skip(1)
            .filter(|a| self.union(&first, a, &tx.txid, "common_input"))
            .count()
    }

    /// Merge a High-confidence change output with the inputs of `tx`.
    pub fn add_change(&mut self, tx: &UtxoTx, change: &ChangeGuess) -> bool {
        if change.confidence < Confidence::High {
            return false;
        }
        let Some(first_input) = tx.input_addresses().first().copied().cloned() else {
            return false;
        };
        let Some(out) = tx
            .outputs
            .iter()
            .find(|o| o.index == change.output)
            .and_then(|o| o.address.clone())
        else {
            return false;
        };
        self.union(&first_input, &out, &tx.txid, "change")
    }

    pub fn same_owner(&mut self, a: &Address, b: &Address) -> bool {
        match (self.index.get(a).copied(), self.index.get(b).copied()) {
            (Some(ia), Some(ib)) => self.root(ia) == self.root(ib),
            _ => a == b,
        }
    }

    /// All addresses known to share an owner with `a` (including `a`).
    pub fn members(&mut self, a: &Address) -> Vec<Address> {
        let Some(&ia) = self.index.get(a) else {
            return vec![a.clone()];
        };
        let ra = self.root(ia);
        let in_cluster: Vec<usize> = (0..self.addresses.len())
            .filter(|&i| self.root(i) == ra)
            .collect();
        in_cluster
            .into_iter()
            .map(|i| self.addresses[i].clone())
            .collect()
    }

    pub fn evidence(&self) -> &[MergeEvidence] {
        &self.evidence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::change::detect_change;
    use crate::btc::fixtures;

    #[test]
    fn common_inputs_merge_with_evidence() {
        let tx = fixtures::tx("change_multi_input");
        let mut c = Clusters::new();
        let merges = c.add_common_inputs(&tx);
        let addrs = tx.input_addresses();
        assert_eq!(merges, addrs.len() - 1);
        assert!(c.same_owner(addrs[0], addrs[addrs.len() - 1]));
        assert_eq!(c.members(addrs[0]).len(), addrs.len());
        assert!(c.evidence().iter().all(|e| e.heuristic == "common_input"));
    }

    #[test]
    fn coinjoin_inputs_never_merge() {
        let tx = fixtures::tx("coinjoin_wabisabi");
        let mut c = Clusters::new();
        assert_eq!(c.add_common_inputs(&tx), 0);
        let addrs = tx.input_addresses();
        assert!(!c.same_owner(addrs[0], addrs[1]));
    }

    #[test]
    fn high_confidence_change_merges() {
        let tx = fixtures::tx("change_type_and_round");
        let mut c = Clusters::new();
        let g = detect_change(&tx).unwrap();
        assert!(c.add_change(&tx, &g));
        let change_addr = tx.outputs[1].address.clone().unwrap();
        let payee = tx.outputs[0].address.clone().unwrap();
        assert!(c.same_owner(tx.input_addresses()[0], &change_addr));
        assert!(!c.same_owner(tx.input_addresses()[0], &payee));
    }
}
