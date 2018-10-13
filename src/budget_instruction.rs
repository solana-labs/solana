use budget::Budget;
use chrono::prelude::{DateTime, Utc};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    /// We send some gossip specific membership information through the vote to shortcut
    /// liveness voting
    /// The version of the ClusterInfo struct that the last_id of this network voted with
    pub version: u64,
    /// The version of the ClusterInfo struct that has the same network configuration as this one
    pub contact_info_version: u64,
    // TODO: add signature of the state here as well
}

/// An instruction to progress the smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Instruction {
    /// Declare and instantiate `Budget`.
    NewBudget(i64, Budget),

    /// Tell a payment plan acknowledge the given `DateTime` has past.
    ApplyTimestamp(DateTime<Utc>),

    /// Tell the budget that the `NewBudget` with `Signature` has been
    /// signed by the containing transaction's `Pubkey`.
    ApplySignature,

    /// Vote for a PoH that is equal to the lastid of this transaction
    NewVote(Vote),
}
