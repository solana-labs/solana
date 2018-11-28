use budget_expr::BudgetExpr;
use chrono::prelude::{DateTime, Utc};

/// A smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Contract {
    /// The number of tokens allocated to the `BudgetExpr` and any transaction fees.
    pub tokens: u64,
    pub budget_expr: BudgetExpr,
}

/// An instruction to progress the smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Instruction {
    /// Declare and instantiate `BudgetExpr`.
    NewBudget(BudgetExpr),

    /// Tell a payment plan acknowledge the given `DateTime` has past.
    ApplyTimestamp(DateTime<Utc>),

    /// Tell the budget that the `NewBudget` with `Signature` has been
    /// signed by the containing transaction's `Pubkey`.
    ApplySignature,
}
