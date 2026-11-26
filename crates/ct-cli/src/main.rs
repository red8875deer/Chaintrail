//! `chaintrail` command-line interface.
//!
//! Every feature lands here first, so it can be tested before the desktop UI exists.

mod labels_cmd;
mod lookup;
mod trace_cmd;

use std::io::Write as _;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

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

    /// Do not read or write the local cache.
    #[arg(long, global = true)]
    no_cache: bool,

    /// Cache file (default: your user data folder, chaintrail/cache.sqlite).
    #[arg(long, global = true, value_name = "PATH")]
    cache: Option<std::path::PathBuf>,

    /// Extra folder of label packs (*.toml) to load after the built-in and user labels.
    #[arg(long, global = true, value_name = "DIR")]
    labels: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Identify which chain an address belongs to and validate its checksum.
    Check { address: String },
    /// Bitcoin lookups and tracing.
    #[command(subcommand)]
    Btc(BtcCommand),
    /// Address labels: who owns an address, according to whom.
    #[command(subcommand)]
    Labels(LabelsCommand),
}

#[derive(Subcommand)]
enum LabelsCommand {
    /// Show every label for an address.
    Lookup { address: String },
    /// List loaded label packs with their sources and licences.
    Packs,
    /// Add your own label (stored in your user data folder, labels/user.toml).
    Add {
        address: String,
        /// Who owns it, e.g. "Example Exchange".
        #[arg(long)]
        entity: String,
        /// exchange, mixer, bridge, dex, defi, swap_service, payment_processor,
        /// wallet_service, mining_pool, gambling, darknet, scam, ransomware, hack,
        /// sanctioned, individual, unknown_service
        #[arg(long)]
        category: String,
        /// low, medium or high.
        #[arg(long, default_value = "medium")]
        confidence: String,
        #[arg(long)]
        note: Option<String>,
        /// Where this can be checked (URL or reference).
        #[arg(long)]
        source: Option<String>,
    },
    /// Download the latest OFAC sanctions list (or read --file) and update the labels.
    UpdateOfac {
        /// Use a local SDN.XML instead of downloading.
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// Where to write the pack (default: your user labels folder).
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
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
    /// Transaction inputs and outputs, where each output went next, and heuristics.
    Tx { txid: String },
    /// Follow funds from a transaction and list where they ended up (or came from).
    Trace {
        /// Transaction to start from.
        txid: String,
        /// Start from this output only.
        #[arg(long)]
        vout: Option<u32>,
        /// Start from the outputs of the transaction paying this address.
        #[arg(long)]
        to: Option<String>,
        /// Trace backwards: where did the funds come from?
        #[arg(long)]
        backward: bool,
        /// Maximum transactions between the start and a lead.
        #[arg(long, default_value_t = 8)]
        max_hops: u32,
        /// Maximum transactions to fetch.
        #[arg(long, default_value_t = 100)]
        max_txs: u32,
        /// Ignore branches smaller than this many BTC (default: 0.1% of the start amount).
        #[arg(long)]
        min_btc: Option<String>,
        /// Do not look up address activity (faster, but busy service wallets are missed).
        #[arg(long)]
        no_address_checks: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let output = match cli.command {
        Command::Check { address } => lookup::check(&address, cli.json)?,
        Command::Labels(cmd) => match cmd {
            LabelsCommand::Lookup { address } => {
                let index = labels_cmd::load_index(cli.labels.as_deref())?;
                labels_cmd::lookup(&index, &address, cli.json)?
            }
            LabelsCommand::Packs => {
                labels_cmd::packs(&labels_cmd::load_index(cli.labels.as_deref())?)?
            }
            LabelsCommand::Add {
                address,
                entity,
                category,
                confidence,
                note,
                source,
            } => labels_cmd::add(labels_cmd::AddArgs {
                address,
                entity,
                category,
                confidence,
                note,
                source,
            })?,
            LabelsCommand::UpdateOfac { file, out } => {
                labels_cmd::update_ofac(file.as_deref(), out.as_deref()).await?
            }
        },
        Command::Btc(cmd) => {
            let provider = bitcoin_provider(cli.no_cache, cli.cache.as_deref())?;
            let provider: &dyn ct_providers::BitcoinProvider = provider.as_ref();
            match cmd {
                BtcCommand::Address { address, pages } => {
                    let labels = labels_cmd::load_index(cli.labels.as_deref())?;
                    lookup::btc_address(provider, &labels, &address, pages, cli.json).await?
                }
                BtcCommand::Tx { txid } => {
                    let labels = labels_cmd::load_index(cli.labels.as_deref())?;
                    lookup::btc_tx(provider, &labels, &txid, cli.json).await?
                }
                BtcCommand::Trace {
                    txid,
                    vout,
                    to,
                    backward,
                    max_hops,
                    max_txs,
                    min_btc,
                    no_address_checks,
                } => {
                    let args = trace_cmd::TraceArgs {
                        txid,
                        vout,
                        to,
                        backward,
                        max_hops,
                        max_txs,
                        min_btc,
                        no_address_checks,
                    };
                    let labels = labels_cmd::load_index(cli.labels.as_deref())?;
                    trace_cmd::btc_trace(provider, &labels, args, cli.json).await?
                }
            }
        }
    };
    emit(&output)
}

/// The free public provider chain, wrapped in the local cache unless disabled.
fn bitcoin_provider(
    no_cache: bool,
    path: Option<&std::path::Path>,
) -> Result<Box<dyn ct_providers::BitcoinProvider>> {
    let live = ct_providers::default_bitcoin_provider()?;
    if no_cache {
        return Ok(Box::new(live));
    }
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => ct_store::Store::default_path()
            .context("no user data folder found; pass --cache PATH or --no-cache")?,
    };
    let store = ct_store::Store::open(&path)
        .with_context(|| format!("opening cache {}", path.display()))?;
    Ok(Box::new(ct_store::CachedBitcoin::new(
        live,
        store,
        ct_store::CachePolicy::default(),
    )))
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
