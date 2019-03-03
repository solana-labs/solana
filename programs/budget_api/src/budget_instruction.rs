use crate::budget_expr::BudgetExpr;
use crate::id;
use chrono::prelude::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction_builder::BuilderInstruction;

/// A smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Contract {
    /// The number of tokens allocated to the `BudgetExpr` and any transaction fees.
    pub tokens: u64,
    pub budget_expr: BudgetExpr,
}

/// An instruction to progress the smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum BudgetInstruction {
    /// Declare and instantiate `BudgetExpr`.
    InitializeAccount(BudgetExpr),

    /// Tell a payment plan acknowledge the given `DateTime` has past.
    ApplyTimestamp(DateTime<Utc>),

    /// Tell the budget that the `InitializeAccount` with `Signature` has been
    /// signed by the containing transaction's `Pubkey`.
    ApplySignature,
}

impl BudgetInstruction {
    pub fn new_initialize_account(contract: Pubkey, expr: BudgetExpr) -> BuilderInstruction {
        let mut keys = vec![];
        if let BudgetExpr::Pay(payment) = &expr {
            keys.push((payment.to, false));
        } else {
            panic!("unsupported Budget instruction");
        }
        keys.push((contract, false));
        BuilderInstruction::new(id(), &BudgetInstruction::InitializeAccount(expr), keys)
    }
}
