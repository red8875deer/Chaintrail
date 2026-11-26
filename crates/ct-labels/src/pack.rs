//! The label pack file format.
//!
//! ```toml
//! [pack]
//! id = "ofac-sdn"
//! title = "OFAC SDN list: digital currency addresses"
//! source = "https://sanctionslistservice.ofac.treas.gov/..."
//! publisher = "US Treasury, Office of Foreign Assets Control"
//! licence = "Public domain (US government work)"
//! as_of = "2026-11-23"
//! category = "sanctioned"      # default for groups
//! confidence = "high"          # default for groups
//!
//! [[group]]
//! entity = "Example Entity"
//! note = "SDN uid 123; program CYBER2"   # optional
//! category = "exchange"                  # optional override
//! confidence = "medium"                  # optional override
//! source = "https://…"                   # optional per-group source
//! bitcoin = ["1…", "bc1…"]
//! ethereum = ["0x…"]
//! tron = ["T…"]
//! ```

use ct_core::{Address, Chain, Confidence, EntityCategory};
use serde::{Deserialize, Serialize};

use crate::error::LabelError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackFile {
    pub pack: PackMeta,
    #[serde(default, rename = "group", skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackMeta {
    /// Short stable identifier; a pack loaded later with the same id replaces this one.
    pub id: String,
    pub title: String,
    /// Where the labels can be checked.
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Licence or terms of the data.
    pub licence: String,
    /// Date of the source data (ISO), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    /// Intermediate collection the data was taken from, e.g. "GraphSense TagPacks (MIT)".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub category: EntityCategory,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub entity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<EntityCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Confidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bitcoin: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ethereum: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tron: Vec<String>,
}

impl Group {
    pub fn addresses(&self) -> impl Iterator<Item = (Chain, &str)> {
        self.bitcoin
            .iter()
            .map(|a| (Chain::Bitcoin, a.as_str()))
            .chain(self.ethereum.iter().map(|a| (Chain::Ethereum, a.as_str())))
            .chain(self.tron.iter().map(|a| (Chain::Tron, a.as_str())))
    }

    /// Add an address to the list for its chain.
    pub fn push(&mut self, address: &Address) {
        let list = match address.chain() {
            Chain::Bitcoin => &mut self.bitcoin,
            Chain::Ethereum => &mut self.ethereum,
            Chain::Tron => &mut self.tron,
        };
        if !list.iter().any(|a| a == address.as_str()) {
            list.push(address.as_str().to_string());
        }
    }
}

/// One validated label, flattened from a pack.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLabel {
    pub address: Address,
    pub entity: String,
    pub category: EntityCategory,
    pub confidence: Confidence,
    pub source: String,
    pub note: Option<String>,
}

impl PackFile {
    pub fn parse(text: &str, name_for_errors: &str) -> Result<PackFile, LabelError> {
        let pack: PackFile = toml::from_str(text).map_err(|e| LabelError::Toml {
            pack: name_for_errors.to_string(),
            detail: e.to_string(),
        })?;
        pack.check_meta()?;
        Ok(pack)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("pack types always serialise")
    }

    fn invalid(&self, detail: impl Into<String>) -> LabelError {
        LabelError::Invalid {
            pack: self.pack.id.clone(),
            detail: detail.into(),
        }
    }

    fn check_meta(&self) -> Result<(), LabelError> {
        let id_ok = !self.pack.id.is_empty()
            && self
                .pack
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !id_ok {
            return Err(self.invalid("id must be lowercase letters, digits and dashes"));
        }
        if self.pack.source.trim().is_empty() || self.pack.licence.trim().is_empty() {
            return Err(self.invalid("source and licence are required"));
        }
        if self.pack.confidence == Confidence::Certain
            || self
                .groups
                .iter()
                .any(|g| g.confidence == Some(Confidence::Certain))
        {
            return Err(self.invalid("labels are attributions and cannot be 'certain'"));
        }
        Ok(())
    }

    /// Validate every address and flatten into labels. Any invalid address fails the whole
    /// pack, so bad data is caught by the tests instead of silently ignored.
    pub fn labels(&self) -> Result<Vec<ParsedLabel>, LabelError> {
        let mut out = Vec::new();
        for g in &self.groups {
            if g.entity.trim().is_empty() {
                return Err(self.invalid("group with empty entity"));
            }
            for (chain, raw) in g.addresses() {
                let address = Address::parse(chain, raw)
                    .map_err(|e| self.invalid(format!("entity {:?}: {raw}: {e}", g.entity)))?;
                out.push(ParsedLabel {
                    address,
                    entity: g.entity.clone(),
                    category: g.category.unwrap_or(self.pack.category),
                    confidence: g.confidence.unwrap_or(self.pack.confidence),
                    source: g.source.clone().unwrap_or_else(|| self.pack.source.clone()),
                    note: g.note.clone(),
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[pack]
id = "sample"
title = "Sample"
source = "https://example.org/list"
licence = "CC0"
category = "exchange"
confidence = "high"

[[group]]
entity = "Example Exchange"
bitcoin = ["1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"]
ethereum = ["0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"]

[[group]]
entity = "Example Mixer"
category = "mixer"
confidence = "medium"
source = "https://example.org/mixer"
tron = ["TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"]
"#;

    #[test]
    fn parses_and_flattens() {
        let p = PackFile::parse(SAMPLE, "sample").unwrap();
        let labels = p.labels().unwrap();
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[0].category, EntityCategory::Exchange);
        assert_eq!(labels[0].source, "https://example.org/list");
        // Ethereum addresses are normalised to lowercase.
        assert_eq!(
            labels[1].address.as_str(),
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"
        );
        assert_eq!(labels[2].category, EntityCategory::Mixer);
        assert_eq!(labels[2].confidence, Confidence::Medium);
        assert_eq!(labels[2].source, "https://example.org/mixer");
    }

    #[test]
    fn round_trips() {
        let p = PackFile::parse(SAMPLE, "sample").unwrap();
        let again = PackFile::parse(&p.to_toml(), "sample").unwrap();
        assert_eq!(p, again);
    }

    #[test]
    fn rejects_bad_data() {
        let bad_addr = SAMPLE.replace(
            "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
            "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb",
        );
        let err = PackFile::parse(&bad_addr, "x")
            .unwrap()
            .labels()
            .unwrap_err();
        assert!(err.to_string().contains("Example Exchange"), "{err}");

        let certain = SAMPLE.replace("confidence = \"high\"", "confidence = \"certain\"");
        assert!(PackFile::parse(&certain, "x").is_err());

        let bad_id = SAMPLE.replace("id = \"sample\"", "id = \"Bad Id\"");
        assert!(PackFile::parse(&bad_id, "x").is_err());

        assert!(PackFile::parse("not toml [", "x").is_err());
    }
}
