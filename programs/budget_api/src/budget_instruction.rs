use crate::budget_expr::BudgetExpr;
use crate::id;
use chrono::prelude::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;

/// A smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Contract {
    /// The number of lamports allocated to the `BudgetExpr` and any transaction fees.
    pub lamports: u64,
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
    pub fn new_initialize_account(contract: &Pubkey, expr: BudgetExpr) -> Instruction {
        let mut keys = vec![];
        if let BudgetExpr::Pay(payment) = &expr {
            keys.push(AccountMeta::new(payment.to, false));
        }
        keys.push(AccountMeta::new(*contract, false));
        Instruction::new(id(), &BudgetInstruction::InitializeAccount(expr), keys)
    }

    pub fn new_apply_timestamp(
        from: &Pubkey,
        contract: &Pubkey,
        to: &Pubkey,
        dt: DateTime<Utc>,
    ) -> Instruction {
        let mut account_metas = vec![
            AccountMeta::new(*from, true),
            AccountMeta::new(*contract, false),
        ];
        if from != to {
            account_metas.push(AccountMeta::new(*to, false));
        }
        Instruction::new(id(), &BudgetInstruction::ApplyTimestamp(dt), account_metas)
    }

    pub fn new_apply_signature(from: &Pubkey, contract: &Pubkey, to: &Pubkey) -> Instruction {
        let mut account_metas = vec![
            AccountMeta::new(*from, true),
            AccountMeta::new(*contract, false),
        ];
        if from != to {
            account_metas.push(AccountMeta::new(*to, false));
        }
        Instruction::new(id(), &BudgetInstruction::ApplySignature, account_metas)
    }
}
