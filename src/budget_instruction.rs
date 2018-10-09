use budget::Budget;
use chrono::prelude::{DateTime, Utc};

/// A smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Contract {
    /// The number of tokens allocated to the `Budget` and any transaction fees.
    pub tokens: i64,
    pub budget: Budget,
}
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
    /// Declare and instantiate `Contract`.
    NewContract(Contract),

    /// Tell a payment plan acknowledge the given `DateTime` has past.
    ApplyTimestamp(DateTime<Utc>),

    /// Tell the payment plan that the `NewContract` with `Signature` has been
    /// signed by the containing transaction's `Pubkey`.
    ApplySignature,

    /// Vote for a PoH that is equal to the lastid of this transaction
    NewVote(Vote),
}
