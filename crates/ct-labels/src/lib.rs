//! Address labels: who owns an address, according to whom.
//!
//! Labels come in *packs*: TOML files with the pack's source, publisher, licence and date,
//! and groups of addresses per entity (`docs/DATA_SOURCES.md`). Built-in packs live in
//! `data/labels/` and are compiled into the binary; users can add their own packs in a
//! folder. Every label becomes an `Attribution` claim with its source, never a fact.

mod builtin;
mod error;
mod index;
pub mod ofac;
pub mod pack;

pub use builtin::builtin_packs;
pub use error::LabelError;
pub use index::{LabelIndex, LabelRecord, PackSummary};
pub use pack::{Group, PackFile, PackMeta};

/// Pack id used for labels the user adds themselves.
pub const USER_PACK_ID: &str = "user";
