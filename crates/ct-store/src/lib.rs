//! Local SQLite storage.
//!
//! Free public APIs are rate limited, so every fetched object is cached on disk and reused.
//! Confirmed blockchain data never changes and is kept forever; volatile data (address
//! balances, the newest page of an address's history, unconfirmed spends) expires after
//! [`CachePolicy::volatile_ttl`]. See [`CachedBitcoin`] for the exact rules.

mod cached;
mod error;
mod store;

pub use cached::{CachePolicy, CacheStats, CachedBitcoin};
pub use error::StoreError;
pub use store::Store;
