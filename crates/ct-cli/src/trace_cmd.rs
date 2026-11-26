//! `btc trace`: follow funds and print leads.

use std::fmt::Write as _;
use std::io::{IsTerminal, Write as _};

use anyhow::Result;
use ct_core::time::format_utc;
use ct_core::{Address, Chain, Claim};
use ct_providers::BitcoinProvider;
use ct_trace::{Direction, FlowRole, Lead, LeadKind, Seed, TraceConfig, TraceReport};

use crate::lookup::btc;

pub struct TraceArgs {
    pub txid: String,
    pub vout: Option<u32>,
    pub to: Option<String>,
    pub backward: bool,
    pub max_hops: u32,
    pub max_txs: u32,
    pub min_btc: Option<String>,
    pub no_address_checks: bool,
}

pub async fn btc_trace(
    p: &dyn BitcoinProvider,
    labels: &dyn ct_core::LabelLookup,
    args: TraceArgs,
    json: bool,
) -> Result<String> {
    let seed = match (args.vout, &args.to) {
        (Some(vout), None) => Seed::Output {
            txid: args.txid.clone(),
            vout,
        },
        (None, Some(to)) => Seed::Payment {
            txid: args.txid.clone(),
            address: Address::parse(Chain::Bitcoin, to)?,
        },
        (None, None) => Seed::Tx {
            txid: args.txid.clone(),
        },
        (Some(_), Some(_)) => anyhow::bail!("use either --vout or --to, not both"),
    };
    let min_amount = args.min_btc.as_deref().map(parse_btc).transpose()?;
    let config = TraceConfig {
        direction: if args.backward {
            Direction::Backward
        } else {
            Direction::Forward
        },
        max_hops: args.max_hops,
        max_txs: args.max_txs,
        min_amount,
        check_addresses: !args.no_address_checks,
        ..Default::default()
    };

    let show_progress = std::io::stderr().is_terminal();
    let mut progress = |pr: ct_trace::Progress| {
        if show_progress {
            let mut e = std::io::stderr().lock();
            let _ = write!(
                e,
                "\r  examined {} txs · {} branches queued · {} leads   ",
                pr.txs_examined, pr.pending, pr.leads
            );
            let _ = e.flush();
        }
    };
    let report = ct_trace::trace_bitcoin(p, labels, seed, config, &mut progress).await?;
    if show_progress {
        eprintln!();
    }

    if json {
        return Ok(serde_json::to_string_pretty(&report)? + "\n");
    }
    render(&report, p.name())
}

/// Unfinished branches listed individually; the rest are summarised.
const SHOW_OPEN: usize = 5;

fn render_lead(out: &mut String, n: usize, l: &Lead, seed_amount: u64) -> Result<()> {
    writeln!(
        out,
        "{n:>3}. {:>18} {:>8}  {}",
        btc(l.amount),
        share(l.amount, seed_amount),
        l.title
    )?;
    let kind = match &l.kind {
        LeadKind::Endpoint => "endpoint",
        LeadKind::AtRest => "at rest",
        LeadKind::Mined => "mined",
        LeadKind::Unexplored { .. } => "unexplored",
    };
    let basis = match &l.claim {
        Claim::Fact => "on-chain fact".to_string(),
        Claim::Inference { heuristic, .. } => format!("inferred ({heuristic})"),
        Claim::Attribution { source, .. } => format!("label from {source}"),
    };
    writeln!(
        out,
        "      {kind} · {} confidence · {basis} · {} hop(s){}",
        l.confidence,
        l.hops,
        l.first_time
            .map(|t| format!(" · {}", format_utc(t)))
            .unwrap_or_default()
    )?;
    if let Some(a) = &l.address {
        writeln!(out, "      address {a}")?;
    }
    if let Some(t) = &l.txid {
        writeln!(out, "      tx      {t}")?;
    }
    match &l.claim {
        Claim::Inference { evidence, .. } => {
            for e in evidence {
                writeln!(out, "      why     {e}")?;
            }
        }
        Claim::Attribution { url: Some(u), .. } => writeln!(out, "      source  {u}")?,
        _ => {}
    }
    Ok(())
}

/// Parse a decimal BTC amount ("0.05") into sats without floating point.
fn parse_btc(s: &str) -> Result<u64> {
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    if frac.len() > 8 || !int.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        anyhow::bail!("not a BTC amount: {s}");
    }
    let int: u64 = if int.is_empty() { 0 } else { int.parse()? };
    let frac: u64 = format!("{frac:0<8}").parse()?;
    Ok(int * 100_000_000 + frac)
}

/// Percentage with two decimals using integer maths.
fn share(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "-".into();
    }
    let bps = (part as u128 * 10_000 / whole as u128) as u64;
    format!("{}.{:02}%", bps / 100, bps % 100)
}

fn render(r: &TraceReport, source: &str) -> Result<String> {
    let mut out = String::new();
    let dir = match r.direction {
        Direction::Forward => "Forward trace (where did it go?)",
        Direction::Backward => "Backward trace (where did it come from?)",
    };
    writeln!(out, "{dir}")?;
    writeln!(out, "Seed      {}", r.seed)?;
    writeln!(out, "Amount    {}", btc(r.seed_amount))?;
    writeln!(
        out,
        "Rule      pro-rata · max {} hops · max {} txs · ignore branches under {}",
        r.config.max_hops,
        r.config.max_txs,
        btc(r.min_amount)
    )?;
    writeln!(
        out,
        "Work      {} transactions, {} address checks via {source}",
        r.stats.txs_examined, r.stats.addresses_checked
    )?;
    writeln!(out)?;

    let heading = match r.direction {
        Direction::Forward => "LEADS: where the funds ended up",
        Direction::Backward => "LEADS: where the funds came from",
    };
    writeln!(out, "{heading}")?;
    let (open, found): (Vec<&Lead>, Vec<&Lead>) = r
        .leads
        .iter()
        .partition(|l| matches!(l.kind, LeadKind::Unexplored { .. }));
    if found.is_empty() {
        writeln!(out, "  (none found within the limits)")?;
    }
    for (n, l) in found.iter().enumerate() {
        render_lead(&mut out, n + 1, l, r.seed_amount)?;
    }

    if !open.is_empty() {
        let total: u64 = open.iter().map(|l| l.amount).sum();
        writeln!(out)?;
        writeln!(
            out,
            "NOT FOLLOWED: {} in {} branch(es), {} of the funds",
            btc(total),
            open.len(),
            share(total, r.seed_amount)
        )?;
        for l in open.iter().take(SHOW_OPEN) {
            writeln!(
                out,
                "  {:>18}  {}  ({})",
                btc(l.amount),
                l.address
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| l.txid.clone().unwrap_or_default()),
                l.title.trim_start_matches("Not followed: ")
            )?;
        }
        if open.len() > SHOW_OPEN {
            writeln!(
                out,
                "  … {} more (use --json for all)",
                open.len() - SHOW_OPEN
            )?;
        }
        writeln!(
            out,
            "  Raise --max-hops / --max-txs, or trace a branch on its own to go further."
        )?;
    }

    if !r.labels_seen.is_empty() {
        writeln!(out)?;
        writeln!(
            out,
            "LABELLED ADDRESSES PASSED THROUGH (not treated as endpoints)"
        )?;
        for l in &r.labels_seen {
            writeln!(
                out,
                "  {}  {} ({}, {} confidence)",
                l.address,
                l.entity,
                l.category.as_str(),
                l.claim.confidence()
            )?;
        }
    }

    writeln!(out)?;
    let leads_total: u64 = r.leads.iter().map(|l| l.amount).sum();
    writeln!(
        out,
        "Accounting  leads {} + fees {} + small branches {} = {}",
        btc(leads_total),
        btc(r.stats.fees),
        btc(r.stats.below_threshold),
        btc(r.accounted())
    )?;
    let changes = r
        .flows
        .iter()
        .filter(|f| matches!(f.role, FlowRole::Change { .. }))
        .count();
    if changes > 0 || r.stats.peel_steps > 0 {
        writeln!(
            out,
            "Change      {changes} hop(s) followed as same-owner change, {} peel-chain step(s)",
            r.stats.peel_steps
        )?;
    }
    if r.stats.budget_exhausted {
        writeln!(
            out,
            "Note        transaction budget used up; raise --max-txs to go further"
        )?;
    }
    writeln!(
        out,
        "Caution     inferred leads are leads, not proof. Verify each one before acting."
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btc_amounts_parse_exactly() {
        assert_eq!(parse_btc("1").unwrap(), 100_000_000);
        assert_eq!(parse_btc("0.05").unwrap(), 5_000_000);
        assert_eq!(parse_btc(".00000001").unwrap(), 1);
        assert!(parse_btc("0.000000001").is_err());
        assert!(parse_btc("1e3").is_err());
    }

    #[test]
    fn shares() {
        assert_eq!(share(1, 4), "25.00%");
        assert_eq!(share(1, 3), "33.33%");
        assert_eq!(share(0, 0), "-");
    }
}
