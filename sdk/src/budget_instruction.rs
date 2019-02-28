use crate::budget_expr::BudgetExpr;
use crate::budget_program;
use crate::pubkey::Pubkey;
use crate::transaction_builder::BuilderInstruction;
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

impl Instruction {
    pub fn new_budget(contract: Pubkey, expr: BudgetExpr) -> BuilderInstruction {
        BuilderInstruction::new(
            budget_program::id(),
            &Instruction::NewBudget(expr),
            vec![(contract, false)],
        )
    }
}
