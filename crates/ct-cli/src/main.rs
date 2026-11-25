//! `chaintrail` command-line interface.
//!
//! Every feature lands here first, so it can be tested before the desktop UI exists.

use std::fmt::Write as _;
use std::io::Write as _;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use ct_core::time::format_utc;
use ct_core::{Address, Amount, Chain, ScriptType, UtxoTx};
use ct_providers::BitcoinProvider;

#[derive(Parser)]
#[command(
    name = "chaintrail",
    version,
    about = "Free, open-source crypto tracing"
)]
struct Cli {
    /// Print machine-readable JSON instead of tables.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Identify which chain an address belongs to and validate its checksum.
    Check { address: String },
    /// Bitcoin lookups.
    #[command(subcommand)]
    Btc(BtcCommand),
}

#[derive(Subcommand)]
enum BtcCommand {
    /// Address balance and recent transactions.
    Address {
        address: String,
        /// Number of 25-transaction pages to fetch.
        #[arg(long, default_value_t = 1)]
        pages: u32,
    },
    /// Transaction inputs and outputs, and where each output went next.
    Tx { txid: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let output = match cli.command {
        Command::Check { address } => check(&address, cli.json)?,
        Command::Btc(cmd) => {
            let provider = ct_providers::default_bitcoin_provider()?;
            match cmd {
                BtcCommand::Address { address, pages } => {
                    btc_address(&provider, &address, pages, cli.json).await?
                }
                BtcCommand::Tx { txid } => btc_tx(&provider, &txid, cli.json).await?,
            }
        }
    };
    emit(&output)
}

/// Write to stdout, treating a closed pipe (e.g. `| head`) as success.
fn emit(text: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => Ok(other?),
    }
}

fn btc(sats: u64) -> String {
    format!("{} BTC", Amount::new(sats as u128, 8))
}

fn script_label(script: Option<&ScriptType>) -> String {
    match script {
        Some(ScriptType::Other(name)) => format!("<{name} script>"),
        Some(s) => format!("<{s:?} script>").to_lowercase(),
        None => "<unknown script>".to_string(),
    }
}

fn check(input: &str, json: bool) -> Result<String> {
    let address = Address::detect(input)?;
    let mut out = String::new();
    if json {
        writeln!(out, "{}", serde_json::to_string_pretty(&address)?)?;
        return Ok(out);
    }
    writeln!(out, "valid {} address: {address}", address.chain())?;
    if let Some(kind) = address.bitcoin_kind() {
        writeln!(out, "type: {kind:?}")?;
    }
    if let Some(checksummed) = address.eip55() {
        writeln!(out, "checksummed: {checksummed}")?;
    }
    Ok(out)
}

async fn btc_address(
    p: &dyn BitcoinProvider,
    input: &str,
    pages: u32,
    json: bool,
) -> Result<String> {
    let address = Address::parse(Chain::Bitcoin, input)?;
    let summary = p.address_summary(&address).await?;

    let mut txs: Vec<UtxoTx> = Vec::new();
    for _ in 0..pages {
        let after = txs.last().map(|t| t.txid.clone());
        let page = p.address_txs(&address, after.as_deref()).await?;
        if page.is_empty() {
            break;
        }
        txs.extend(page);
    }

    let mut out = String::new();
    if json {
        let doc = serde_json::json!({ "summary": summary, "transactions": txs });
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
        return Ok(out);
    }

    writeln!(out, "Address   {address}")?;
    writeln!(out, "Source    {}", p.name())?;
    writeln!(out, "Txs       {}", summary.tx_count)?;
    writeln!(out, "Received  {}", btc(summary.received as u64))?;
    writeln!(out, "Sent      {}", btc(summary.spent as u64))?;
    writeln!(out, "Balance   {}", btc(summary.balance() as u64))?;
    if summary.unconfirmed_tx_count > 0 {
        writeln!(
            out,
            "Pending   {} unconfirmed tx(s), net {} sats",
            summary.unconfirmed_tx_count, summary.unconfirmed_delta
        )?;
    }
    writeln!(out)?;
    writeln!(out, "{:<24} {:>20}  txid", "time", "net change")?;
    for tx in &txs {
        let received: u64 = tx
            .outputs
            .iter()
            .filter(|o| o.address.as_ref() == Some(&address))
            .map(|o| o.value)
            .sum();
        let sent: u64 = tx
            .inputs
            .iter()
            .filter(|i| i.address.as_ref() == Some(&address))
            .map(|i| i.value)
            .sum();
        let net = if received >= sent {
            format!("+{}", Amount::new((received - sent) as u128, 8))
        } else {
            format!("-{}", Amount::new((sent - received) as u128, 8))
        };
        let time = tx
            .block
            .as_ref()
            .map(|b| format_utc(b.time))
            .unwrap_or_default();
        writeln!(out, "{time:<24} {net:>20}  {}", tx.txid)?;
    }
    if !txs.is_empty() && (txs.len() as u64) < summary.tx_count {
        writeln!(
            out,
            "… showing {} of {} (use --pages)",
            txs.len(),
            summary.tx_count
        )?;
    }
    Ok(out)
}

async fn btc_tx(p: &dyn BitcoinProvider, txid: &str, json: bool) -> Result<String> {
    let tx = p.tx(txid).await?;
    let spends = p
        .outspends(&tx.txid)
        .await
        .context("fetching output spends")?;
    if spends.len() != tx.outputs.len() {
        bail!(
            "{} returned {} spend records for {} outputs",
            p.name(),
            spends.len(),
            tx.outputs.len()
        );
    }

    let mut out = String::new();
    if json {
        let doc = serde_json::json!({ "transaction": tx, "outspends": spends });
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
        return Ok(out);
    }

    writeln!(out, "Tx       {}", tx.txid)?;
    match &tx.block {
        Some(b) => writeln!(out, "Block    {} ({})", b.height, format_utc(b.time))?,
        None => writeln!(out, "Block    unconfirmed")?,
    }
    writeln!(out, "Fee      {} ({} vB)", btc(tx.fee), tx.vsize)?;
    writeln!(out)?;
    writeln!(
        out,
        "Inputs ({}, total {})",
        tx.inputs.len(),
        btc(tx.total_in())
    )?;
    for i in &tx.inputs {
        if i.is_coinbase {
            writeln!(out, "  coinbase (newly mined)")?;
            continue;
        }
        let who = i
            .address
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| script_label(i.script_type.as_ref()));
        writeln!(out, "  {:>18}  {who}", btc(i.value))?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "Outputs ({}, total {})",
        tx.outputs.len(),
        btc(tx.total_out())
    )?;
    for (o, s) in tx.outputs.iter().zip(&spends) {
        let who = o
            .address
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| script_label(Some(&o.script_type)));
        let next = if !o.script_type.is_spendable() {
            "data (unspendable)".to_string()
        } else if s.spent {
            let at = s
                .block
                .as_ref()
                .map(|b| format!(" in block {}", b.height))
                .unwrap_or_else(|| " (unconfirmed)".into());
            format!("spent by {}{at}", s.spending_txid.as_deref().unwrap_or("?"))
        } else {
            "UNSPENT (funds at rest)".to_string()
        };
        writeln!(out, "  #{:<3} {:>18}  {who}", o.index, btc(o.value))?;
        writeln!(out, "        → {next}")?;
    }
    Ok(out)
}
