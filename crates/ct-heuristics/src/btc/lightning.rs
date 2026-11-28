//! Lightning channel closes (`docs/TRACING.md` §3.3b).
//!
//! A channel is a 2-of-2 multisig output. While it is open, payments move off-chain, so the
//! balances its close pays out are not the coins that went in: tracing through a close is an
//! allocation, never a fact.

use ct_core::UtxoTx;
use serde::{Deserialize, Serialize};

use super::spendable_outputs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelClose {
    /// Both parties signed a closing transaction (at most two outputs).
    Mutual,
    /// One party broadcast its commitment transaction (obscured commitment number in the
    /// locktime and sequence).
    Force,
}

impl ChannelClose {
    pub fn describe(self) -> &'static str {
        match self {
            ChannelClose::Mutual => "likely Lightning channel close (mutual)",
            ChannelClose::Force => "likely Lightning channel close (force close)",
        }
    }
}

/// A witness script `2 <key> <key> 2 OP_CHECKMULTISIG`, as Esplora prints it.
fn is_two_of_two(asm: &str) -> bool {
    let t: Vec<&str> = asm.split_whitespace().collect();
    t.len() == 7
        && t[0] == "OP_PUSHNUM_2"
        && t[1] == "OP_PUSHBYTES_33"
        && t[3] == "OP_PUSHBYTES_33"
        && t[5] == "OP_PUSHNUM_2"
        && t[6] == "OP_CHECKMULTISIG"
}

/// Whether `tx` closes a Lightning channel: its one input spends a 2-of-2 multisig, and it
/// is a commitment transaction (BOLT 3 locktime/sequence markers) or pays at most two
/// outputs. Other 2-of-2 wallets exist, so callers treat this as likely, not certain.
pub fn channel_close(tx: &UtxoTx) -> Option<ChannelClose> {
    let [input] = tx.inputs.as_slice() else {
        return None;
    };
    if !is_two_of_two(input.witness_script.as_deref()?) {
        return None;
    }
    if tx.locktime >> 24 == 0x20 && input.sequence >> 24 == 0x80 {
        return Some(ChannelClose::Force);
    }
    (spendable_outputs(tx).count() <= 2).then_some(ChannelClose::Mutual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::fixtures;

    #[test]
    fn real_mutual_close() {
        // Channel 812449x1696x0 (raspinode / CoinGate), closed in 4f5e53ec….
        assert_eq!(
            channel_close(&fixtures::tx("lightning_mutual_close")),
            Some(ChannelClose::Mutual)
        );
        for name in [
            "batch_payout_156out",
            "change_type_and_round",
            "coinjoin_joinmarket",
        ] {
            assert_eq!(channel_close(&fixtures::tx(name)), None, "{name}");
        }
    }
}
