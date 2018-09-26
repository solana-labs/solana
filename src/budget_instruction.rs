use budget::Budget;
use chrono::prelude::{DateTime, Utc};

/// A smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Contract {
    /// The number of tokens allocated to the `Budget` and any transaction fees.
    pub tokens: i64,
    pub budget: Budget,
}

/// An instruction to progress the smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Instruction {
    /// Declare and instantiate `Budget`.
    NewBudget(Budget),

    /// Tell a payment plan acknowledge the given `DateTime` has past.
    ApplyTimestamp(DateTime<Utc>),

    /// Tell the budget that the `NewBudget` with `Signature` has been
    /// signed by the containing transaction's `Pubkey`.
    ApplySignature,
}
