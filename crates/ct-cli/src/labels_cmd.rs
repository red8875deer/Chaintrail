//! `labels`: look up, list, add and update address labels.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use ct_core::{Address, Confidence, EntityCategory};
use ct_labels::LabelIndex;

/// Built-in packs, then the user's folder, then any extra folder given on the command line.
pub fn load_index(extra: Option<&Path>) -> Result<LabelIndex> {
    let (index, problems) = ct_service::labels::load_index(extra).context("loading labels")?;
    for p in problems {
        eprintln!("warning: {p}");
    }
    Ok(index)
}

pub fn lookup(index: &LabelIndex, input: &str, json: bool) -> Result<String> {
    let address = Address::detect(input)?;
    let labels = index.all(&address);
    let mut out = String::new();
    if json {
        let rows: Vec<_> = labels
            .iter()
            .map(|r| {
                serde_json::json!({
                    "entity": r.entity, "category": r.category, "confidence": r.confidence,
                    "pack": r.pack_id, "source": r.source, "as_of": r.as_of, "note": r.note,
                })
            })
            .collect();
        writeln!(out, "{}", serde_json::to_string_pretty(&rows)?)?;
        return Ok(out);
    }
    writeln!(out, "{} address {address}", address.chain())?;
    if labels.is_empty() {
        writeln!(
            out,
            "No labels. (Absence of a label means unknown, not innocent.)"
        )?;
    }
    for r in labels {
        writeln!(
            out,
            "  {} [{:?}] · {} confidence · pack {}{}",
            r.entity,
            r.category,
            r.confidence,
            r.pack_id,
            r.as_of
                .as_deref()
                .map(|d| format!(" (as of {d})"))
                .unwrap_or_default()
        )?;
        if let Some(n) = &r.note {
            writeln!(out, "    note   {n}")?;
        }
        writeln!(out, "    source {}", r.source)?;
    }
    Ok(out)
}

pub fn packs(index: &LabelIndex) -> Result<String> {
    let mut out = String::new();
    writeln!(
        out,
        "{} labelled addresses in {} packs",
        index.len(),
        index.packs().len()
    )?;
    for p in index.packs() {
        writeln!(
            out,
            "  {:<28} {:>7} labels  {}{}",
            p.meta.id,
            p.labels,
            p.meta.title,
            p.meta
                .as_of
                .as_deref()
                .map(|d| format!(" (as of {d})"))
                .unwrap_or_default()
        )?;
        writeln!(
            out,
            "  {:<28} {:>7}         source {}",
            "", "", p.meta.source
        )?;
        writeln!(
            out,
            "  {:<28} {:>7}         licence {} · loaded from {}",
            "", "", p.meta.licence, p.origin
        )?;
    }
    Ok(out)
}

pub struct AddArgs {
    pub address: String,
    pub entity: String,
    pub category: String,
    pub confidence: String,
    pub note: Option<String>,
    pub source: Option<String>,
}

/// Add a label to the user's own pack (`<data dir>/labels/user.toml`).
pub fn add(args: AddArgs) -> Result<String> {
    let address = Address::detect(&args.address)?;
    let category: EntityCategory = parse_enum(&args.category, "category")?;
    let confidence: Confidence = parse_enum(&args.confidence, "confidence")?;
    let dir = ct_service::labels::user_labels_dir().context("no user data folder found")?;
    let path = ct_service::labels::add_user_label(
        &dir,
        &address,
        &args.entity,
        category,
        confidence,
        args.note,
        args.source,
    )?;
    Ok(format!(
        "Labelled {address} as {} ({category:?}, {confidence}) in {}\n",
        args.entity,
        path.display()
    ))
}

fn parse_enum<T: serde::de::DeserializeOwned>(s: &str, what: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(
        s.to_lowercase().replace([' ', '-'], "_"),
    ))
    .with_context(|| format!("unknown {what}: {s}"))
}

/// Build the OFAC pack from a local `SDN.XML` or a fresh download, and write it.
pub async fn update_ofac(file: Option<&Path>, out: Option<&Path>) -> Result<String> {
    let xml = match file {
        Some(f) => {
            std::fs::read_to_string(f).with_context(|| format!("reading {}", f.display()))?
        }
        None => ct_service::labels::download_ofac().await?,
    };
    let path = match out {
        Some(p) => p.to_path_buf(),
        None => ct_service::labels::user_labels_dir()
            .context("no user data folder found; use --out")?
            .join("ofac-sdn.toml"),
    };
    let (pack, stats) = ct_service::labels::write_ofac_pack(&xml, &path)?;
    let mut msg = format!(
        "OFAC SDN list of {}: {} addresses from {} entries written to {}\n",
        pack.pack.as_of.as_deref().unwrap_or("?"),
        stats.addresses,
        stats.entries,
        path.display()
    );
    if !stats.unsupported.is_empty() {
        let skipped: Vec<String> = stats
            .unsupported
            .iter()
            .map(|(c, n)| format!("{c} {n}"))
            .collect();
        writeln!(
            msg,
            "Skipped (chains not supported yet): {}",
            skipped.join(", ")
        )?;
    }
    for r in &stats.reassigned {
        writeln!(msg, "Kept on the chain its format shows: {r}")?;
    }
    for i in &stats.invalid {
        writeln!(msg, "Skipped invalid address: {i}")?;
    }
    Ok(msg)
}
