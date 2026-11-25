//! Chain data adapters.
//!
//! Each data source implements a per-chain trait and converts its API responses into
//! `ct-core` types. Sources are free public APIs by default (see `docs/DATA_SOURCES.md`);
//! every adapter takes a base URL so a self-hosted node can be used instead.

pub mod bitcoin;
pub mod error;
pub mod esplora;
pub mod http;

pub use bitcoin::{BitcoinProvider, FallbackBitcoin};
pub use error::{ProviderError, Result};
pub use esplora::Esplora;
pub use http::{HttpClient, HttpConfig};

/// A Bitcoin provider using mempool.space with blockstream.info as fallback,
/// sharing one rate-limited HTTP client.
pub fn default_bitcoin_provider() -> Result<FallbackBitcoin> {
    let http = HttpClient::new(HttpConfig::default())?;
    Ok(FallbackBitcoin::new(vec![
        Box::new(Esplora::mempool_space(http.clone())),
        Box::new(Esplora::blockstream(http)),
    ]))
}
