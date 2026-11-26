//! In-memory label index.

use std::collections::HashMap;
use std::path::Path;

use ct_core::{Address, Claim, Confidence, EntityCategory, Label, LabelLookup};

use crate::USER_PACK_ID;
use crate::error::LabelError;
use crate::pack::{PackFile, PackMeta};

/// A label with the pack it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRecord {
    pub address: Address,
    pub entity: String,
    pub category: EntityCategory,
    pub confidence: Confidence,
    /// URL or reference where the label can be checked.
    pub source: String,
    pub note: Option<String>,
    pub pack_id: String,
    pub as_of: Option<String>,
}

impl LabelRecord {
    pub fn to_label(&self) -> Label {
        Label {
            address: self.address.clone(),
            entity: self.entity.clone(),
            category: self.category,
            claim: Claim::Attribution {
                source: self.pack_id.clone(),
                confidence: self.confidence,
                as_of: self.as_of.clone(),
                url: Some(self.source.clone()),
            },
            note: self.note.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackSummary {
    pub meta: PackMeta,
    pub labels: usize,
    /// Where it was loaded from: "built-in" or a file path.
    pub origin: String,
}

/// All loaded labels, indexed by address.
#[derive(Debug, Default)]
pub struct LabelIndex {
    packs: Vec<PackSummary>,
    by_address: HashMap<Address, Vec<LabelRecord>>,
}

impl LabelIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Index of the packs compiled into the binary.
    pub fn with_builtin() -> Result<Self, LabelError> {
        let mut index = Self::new();
        for (name, text) in crate::builtin_packs() {
            index.add_pack(PackFile::parse(text, name)?, "built-in")?;
        }
        Ok(index)
    }

    /// Add a pack. A pack with the same id as an existing one replaces it (so a freshly
    /// downloaded OFAC list replaces the built-in snapshot).
    pub fn add_pack(&mut self, pack: PackFile, origin: &str) -> Result<usize, LabelError> {
        let labels = pack.labels()?;
        let id = pack.pack.id.clone();
        if self.packs.iter().any(|p| p.meta.id == id) {
            self.packs.retain(|p| p.meta.id != id);
            for list in self.by_address.values_mut() {
                list.retain(|r| r.pack_id != id);
            }
            self.by_address.retain(|_, list| !list.is_empty());
        }
        let count = labels.len();
        for l in labels {
            let record = LabelRecord {
                address: l.address.clone(),
                entity: l.entity,
                category: l.category,
                confidence: l.confidence,
                source: l.source,
                note: l.note,
                pack_id: id.clone(),
                as_of: pack.pack.as_of.clone(),
            };
            let list = self.by_address.entry(l.address).or_default();
            if !list.contains(&record) {
                list.push(record);
            }
        }
        self.packs.push(PackSummary {
            meta: pack.pack,
            labels: count,
            origin: origin.to_string(),
        });
        Ok(count)
    }

    /// Load every `*.toml` pack in `dir` (sorted by file name). A missing folder is fine.
    pub fn load_dir(&mut self, dir: &Path) -> Result<usize, LabelError> {
        let io = |e| LabelError::Io {
            path: dir.display().to_string(),
            source: e,
        };
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(io(e)),
        };
        let mut paths: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .collect();
        paths.sort();
        let mut total = 0;
        for p in paths {
            let text = std::fs::read_to_string(&p).map_err(|e| LabelError::Io {
                path: p.display().to_string(),
                source: e,
            })?;
            let name = p.display().to_string();
            total += self.add_pack(PackFile::parse(&text, &name)?, &name)?;
        }
        Ok(total)
    }

    /// Every label for `address`, best first.
    pub fn all(&self, address: &Address) -> Vec<&LabelRecord> {
        let mut v: Vec<&LabelRecord> = self
            .by_address
            .get(address)
            .map(|l| l.iter().collect())
            .unwrap_or_default();
        v.sort_by_key(|r| rank(r));
        v
    }

    pub fn packs(&self) -> &[PackSummary] {
        &self.packs
    }

    /// Number of labelled addresses.
    pub fn len(&self) -> usize {
        self.by_address.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_address.is_empty()
    }
}

/// Sort key: strongest confidence, then sanctions (most important to surface), then the
/// user's own labels, then load order.
fn rank(r: &LabelRecord) -> (std::cmp::Reverse<Confidence>, bool, bool) {
    (
        std::cmp::Reverse(r.confidence),
        r.category != EntityCategory::Sanctioned,
        r.pack_id != USER_PACK_ID,
    )
}

impl LabelLookup for LabelIndex {
    fn lookup(&self, address: &Address) -> Option<Label> {
        self.all(address).first().map(|r| r.to_label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_core::Chain;

    fn pack(id: &str, entity: &str, category: &str, confidence: &str) -> PackFile {
        PackFile::parse(
            &format!(
                r#"
[pack]
id = "{id}"
title = "t"
source = "https://example.org/{id}"
licence = "CC0"
category = "{category}"
confidence = "{confidence}"
[[group]]
entity = "{entity}"
bitcoin = ["1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"]
"#
            ),
            id,
        )
        .unwrap()
    }

    fn genesis() -> Address {
        Address::parse(Chain::Bitcoin, "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa").unwrap()
    }

    #[test]
    fn best_label_wins() {
        let mut i = LabelIndex::new();
        i.add_pack(pack("a", "Low One", "exchange", "low"), "test")
            .unwrap();
        i.add_pack(pack("b", "Sanctioned One", "sanctioned", "high"), "test")
            .unwrap();
        i.add_pack(pack("c", "Exchange One", "exchange", "high"), "test")
            .unwrap();
        let best = i.lookup(&genesis()).unwrap();
        assert_eq!(best.entity, "Sanctioned One");
        assert!(matches!(best.claim, Claim::Attribution { ref source, .. } if source == "b"));
        assert_eq!(i.all(&genesis()).len(), 3);
        assert_eq!(i.len(), 1);
    }

    #[test]
    fn user_labels_win_ties() {
        let mut i = LabelIndex::new();
        i.add_pack(pack("c", "Exchange One", "exchange", "high"), "test")
            .unwrap();
        i.add_pack(pack(USER_PACK_ID, "My Label", "exchange", "high"), "test")
            .unwrap();
        assert_eq!(i.lookup(&genesis()).unwrap().entity, "My Label");
    }

    #[test]
    fn same_id_replaces() {
        let mut i = LabelIndex::new();
        i.add_pack(pack("ofac-sdn", "Old", "sanctioned", "high"), "built-in")
            .unwrap();
        i.add_pack(pack("ofac-sdn", "New", "sanctioned", "high"), "download")
            .unwrap();
        assert_eq!(i.all(&genesis()).len(), 1);
        assert_eq!(i.lookup(&genesis()).unwrap().entity, "New");
        assert_eq!(i.packs().len(), 1);
        assert_eq!(i.packs()[0].origin, "download");
    }

    #[test]
    fn loads_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("x.toml"),
            pack("x", "X", "mixer", "medium").to_toml(),
        )
        .unwrap();
        std::fs::write(dir.path().join("ignore.txt"), "not a pack").unwrap();
        let mut i = LabelIndex::new();
        assert_eq!(i.load_dir(dir.path()).unwrap(), 1);
        assert_eq!(
            i.lookup(&genesis()).unwrap().category,
            EntityCategory::Mixer
        );
        assert_eq!(i.load_dir(&dir.path().join("missing")).unwrap(), 0);
    }
}
