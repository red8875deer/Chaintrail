//! ChainTrail domain model.
//!
//! Pure types with no I/O. Every other crate converts its data into these types.
//! See `docs/ARCHITECTURE.md` for how they fit together.

pub mod address;
pub mod amount;
pub mod asset;
pub mod chain;
pub mod claim;
pub mod entity;
pub mod time;
pub mod transfer;
pub mod utxo;

pub use address::{Address, AddressError};
pub use amount::Amount;
pub use asset::Asset;
pub use chain::Chain;
pub use claim::{Claim, Confidence};
pub use entity::{EntityCategory, Label};
pub use transfer::{Transfer, TransferKind};
pub use utxo::{AddressSummary, BlockRef, OutputSpend, ScriptType, TxInput, TxOutput, UtxoTx};
