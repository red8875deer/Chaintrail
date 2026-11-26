//! Address label lookup. The real label dataset arrives in Phase 2; the engine only needs
//! this trait.

use std::collections::HashMap;

pub use ct_core::LabelLookup;
use ct_core::{Address, Label};

/// No labels: endpoints are found by behaviour only.
pub struct NoLabels;

impl LabelLookup for NoLabels {
    fn lookup(&self, _address: &Address) -> Option<Label> {
        None
    }
}

/// Labels held in memory (tests, user-supplied lists).
#[derive(Default)]
pub struct MemoryLabels {
    labels: HashMap<Address, Label>,
}

impl MemoryLabels {
    pub fn new(labels: impl IntoIterator<Item = Label>) -> Self {
        MemoryLabels {
            labels: labels.into_iter().map(|l| (l.address.clone(), l)).collect(),
        }
    }
}

impl LabelLookup for MemoryLabels {
    fn lookup(&self, address: &Address) -> Option<Label> {
        self.labels.get(address).cloned()
    }
}
