//! Importer for the US Treasury OFAC SDN list (`SDN.XML`).
//!
//! The list names sanctioned people and entities; some entries carry
//! "Digital Currency Address - <CURRENCY>" identifiers. Addresses on supported chains become
//! a `sanctioned` label pack. Everything skipped is counted, so nothing disappears silently.

use std::collections::BTreeMap;

use ct_core::{Address, Chain, Confidence, EntityCategory};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::LabelError;
use crate::pack::{Group, PackFile, PackMeta};

/// Official download location (redirects to the current file).
pub const OFAC_SDN_URL: &str =
    "https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/SDN.XML";

const ID_PREFIX: &str = "Digital Currency Address - ";

#[derive(Debug, Default, Clone, PartialEq)]
pub struct OfacStats {
    /// SDN entries with at least one supported address.
    pub entries: usize,
    pub addresses: usize,
    /// Addresses on chains ChainTrail does not support yet, by OFAC currency code.
    pub unsupported: BTreeMap<String, usize>,
    /// Addresses that failed validation, with the reason.
    pub invalid: Vec<String>,
    /// Addresses listed under one currency whose format belongs to another supported chain
    /// (an error in the source data). Kept on the chain the format shows, with a note.
    pub reassigned: Vec<String>,
}

#[derive(Default)]
struct Entry {
    uid: String,
    first: String,
    last: String,
    kind: String,
    programs: Vec<String>,
    ids: Vec<(String, String)>,
}

/// Build the `ofac-sdn` label pack from the text of `SDN.XML`.
pub fn pack_from_sdn_xml(xml: &str) -> Result<(PackFile, OfacStats), LabelError> {
    let err = |e: &dyn std::fmt::Display| LabelError::Ofac(e.to_string());
    // No `trim_text`: it would also trim around `&amp;` and turn "A & B" into "A&B". Values
    // are trimmed once complete instead.
    let mut reader = Reader::from_str(xml);

    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut publish_date = None;
    let mut entry = Entry::default();
    let mut id = (String::new(), String::new());
    let mut entries: Vec<Entry> = Vec::new();

    loop {
        match reader.read_event().map_err(|e| err(&e))? {
            Event::Start(e) => {
                path.push(String::from_utf8_lossy(e.local_name().as_ref()).into_owned());
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.decode().map_err(|e| err(&e))?),
            Event::GeneralRef(r) => {
                if let Some(c) = r.resolve_char_ref().map_err(|e| err(&e))? {
                    text.push(c);
                } else {
                    text.push_str(match r.decode().map_err(|e| err(&e))?.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(_) => {
                let names: Vec<&str> = path.iter().map(String::as_str).collect();
                let value = text.trim().to_string();
                match names.as_slice() {
                    [.., "publshInformation", "Publish_Date"] => publish_date = Some(value),
                    [.., "sdnEntry", "uid"] => entry.uid = value,
                    [.., "sdnEntry", "firstName"] => entry.first = value,
                    [.., "sdnEntry", "lastName"] => entry.last = value,
                    [.., "sdnEntry", "sdnType"] => entry.kind = value,
                    [.., "sdnEntry", "programList", "program"] => entry.programs.push(value),
                    [.., "sdnEntry", "idList", "id", "idType"] => id.0 = value,
                    [.., "sdnEntry", "idList", "id", "idNumber"] => id.1 = value,
                    [.., "sdnEntry", "idList", "id"] => {
                        let done = std::mem::take(&mut id);
                        if done.0.starts_with(ID_PREFIX) {
                            entry.ids.push(done);
                        }
                    }
                    [.., "sdnEntry"] => {
                        let done = std::mem::take(&mut entry);
                        if !done.ids.is_empty() {
                            entries.push(done);
                        }
                    }
                    _ => {}
                }
                path.pop();
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }

    let as_of = publish_date
        .as_deref()
        .and_then(us_date_to_iso)
        .ok_or_else(|| LabelError::Ofac("missing or malformed Publish_Date".into()))?;

    let mut stats = OfacStats::default();
    let mut groups = Vec::new();
    entries.sort_by_key(|e| e.uid.parse::<u64>().unwrap_or(u64::MAX));
    for e in entries {
        let entity = [e.first.as_str(), e.last.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        let mut group = Group {
            entity,
            note: Some(format!(
                "SDN uid {}; {}; programs: {}",
                e.uid,
                if e.kind.is_empty() { "Entity" } else { &e.kind },
                e.programs.join(", ")
            )),
            ..Default::default()
        };
        for (id_type, raw) in &e.ids {
            let currency = id_type.trim_start_matches(ID_PREFIX).trim().to_string();
            let parsed = match currency.as_str() {
                "XBT" => Some(Address::parse(Chain::Bitcoin, raw)),
                "ETH" => Some(Address::parse(Chain::Ethereum, raw)),
                "TRX" => Some(Address::parse(Chain::Tron, raw)),
                // Stablecoins live on several chains; the address format tells which.
                "USDT" | "USDC" => Some(Address::detect(raw)),
                _ => None,
            };
            match parsed {
                None => *stats.unsupported.entry(currency).or_default() += 1,
                Some(Ok(address)) => {
                    group.push(&address);
                    stats.addresses += 1;
                }
                Some(Err(err)) => match Address::detect(raw) {
                    Ok(address) => {
                        let msg = format!(
                            "listed under {currency} but is a {} address",
                            address.chain()
                        );
                        stats.reassigned.push(format!("uid {} {raw}: {msg}", e.uid));
                        if let Some(note) = group.note.as_mut() {
                            note.push_str(&format!("; {raw} {msg}"));
                        }
                        group.push(&address);
                        stats.addresses += 1;
                    }
                    Err(_) => stats
                        .invalid
                        .push(format!("uid {} {currency} {raw}: {err}", e.uid)),
                },
            }
        }
        if group.addresses().next().is_some() {
            stats.entries += 1;
            groups.push(group);
        }
    }

    let pack = PackFile {
        pack: PackMeta {
            id: "ofac-sdn".into(),
            title: "OFAC Specially Designated Nationals (SDN) list: digital currency addresses"
                .into(),
            source: OFAC_SDN_URL.into(),
            publisher: Some(
                "U.S. Department of the Treasury, Office of Foreign Assets Control".into(),
            ),
            licence: "Public domain (work of the U.S. federal government)".into(),
            as_of: Some(as_of),
            via: None,
            description: Some(
                "Addresses OFAC lists as belonging to sanctioned persons or entities. \
                 Dealing with them may be prohibited."
                    .into(),
            ),
            category: EntityCategory::Sanctioned,
            confidence: Confidence::High,
        },
        groups,
    };
    Ok((pack, stats))
}

/// `11/23/2026` → `2026-11-23`.
fn us_date_to_iso(s: &str) -> Option<String> {
    let mut it = s.trim().split('/');
    let (m, d, y) = (it.next()?, it.next()?, it.next()?);
    let (m, d, y): (u32, u32, u32) = (m.parse().ok()?, d.parse().ok()?, y.parse().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| format!("{y:04}-{m:02}-{d:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" standalone="yes"?>
<sdnList xmlns="https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/XML">
  <publshInformation><Publish_Date>11/23/2026</Publish_Date><Record_Count>3</Record_Count></publshInformation>
  <sdnEntry>
    <uid>2</uid><lastName>NO ADDRESSES CORP</lastName><sdnType>Entity</sdnType>
    <programList><program>CUBA</program></programList>
  </sdnEntry>
  <sdnEntry>
    <uid>1</uid><firstName>Jane</firstName><lastName>DOE &amp; CO</lastName><sdnType>Individual</sdnType>
    <programList><program>CYBER2</program><program>RUSSIA-EO14024</program></programList>
    <idList>
      <id><uid>9</uid><idType>Gender</idType><idNumber>Female</idNumber></id>
      <id><uid>10</uid><idType>Digital Currency Address - XBT</idType><idNumber>12cbQLTFMXRnSzktFkuoG3eHoMeFtpTu3S</idNumber></id>
      <id><uid>11</uid><idType>Digital Currency Address - ETH</idType><idNumber>0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045</idNumber></id>
      <id><uid>12</uid><idType>Digital Currency Address - USDT</idType><idNumber>TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t</idNumber></id>
      <id><uid>13</uid><idType>Digital Currency Address - XMR</idType><idNumber>4AdUndXHHZ6cfufTMvppY6JwXNouMBzSkbLYfpAV5Usx3skxNgYeYTRj5UzqtReoS44qo9mtmXCqY45DJ852K5Jv2684Rge</idNumber></id>
      <id><uid>14</uid><idType>Digital Currency Address - XBT</idType><idNumber>1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb</idNumber></id>
      <id><uid>15</uid><idType>Digital Currency Address - XBT</idType><idNumber>TUCsTq7TofTCJRRoHk6RvhMoS2mJLm5Yzq</idNumber></id>
    </idList>
  </sdnEntry>
</sdnList>"#;

    #[test]
    fn parses_sample() {
        let (pack, stats) = pack_from_sdn_xml(SAMPLE).unwrap();
        assert_eq!(pack.pack.as_of.as_deref(), Some("2026-11-23"));
        assert_eq!(pack.groups.len(), 1);
        let g = &pack.groups[0];
        assert_eq!(g.entity, "Jane DOE & CO");
        assert!(
            g.note
                .as_deref()
                .unwrap()
                .contains("CYBER2, RUSSIA-EO14024")
        );
        assert_eq!(g.bitcoin, vec!["12cbQLTFMXRnSzktFkuoG3eHoMeFtpTu3S"]);
        assert_eq!(
            g.ethereum,
            vec!["0xd8da6bf26964af9d7eed9e03e53415d37aa96045"]
        );
        assert_eq!(
            g.tron,
            vec![
                "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t",
                "TUCsTq7TofTCJRRoHk6RvhMoS2mJLm5Yzq"
            ]
        );
        assert_eq!(stats.reassigned.len(), 1);
        assert!(
            g.note
                .as_deref()
                .unwrap()
                .contains("listed under XBT but is a tron address")
        );
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.addresses, 4);
        assert_eq!(stats.unsupported.get("XMR"), Some(&1));
        assert_eq!(stats.invalid.len(), 1);
        // The generated pack is itself valid.
        assert_eq!(pack.labels().unwrap().len(), 4);
    }

    #[test]
    fn dates() {
        assert_eq!(us_date_to_iso("1/5/2026").as_deref(), Some("2026-01-05"));
        assert_eq!(us_date_to_iso("13/5/2026"), None);
        assert_eq!(us_date_to_iso("garbage"), None);
    }
}
