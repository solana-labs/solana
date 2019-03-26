use crate::budget_expr::BudgetExpr;
use crate::budget_state::BudgetState;
use crate::id;
use bincode::serialized_size;
use chrono::prelude::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;

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
    fn new_initialize_account(contract: &Pubkey, expr: BudgetExpr) -> Instruction {
        let mut keys = vec![];
        if let BudgetExpr::Pay(payment) = &expr {
            keys.push(AccountMeta::new(payment.to, false));
        }
        keys.push(AccountMeta::new(*contract, false));
        Instruction::new(id(), &BudgetInstruction::InitializeAccount(expr), keys)
    }

    pub fn new_account(
        from: &Pubkey,
        contract: &Pubkey,
        lamports: u64,
        expr: BudgetExpr,
    ) -> Vec<Instruction> {
        if !expr.verify(lamports) {
            panic!("invalid budget expression");
        }
        let space = serialized_size(&BudgetState::new(expr.clone())).unwrap();
        vec![
            SystemInstruction::new_program_account(&from, contract, lamports, space, &id()),
            BudgetInstruction::new_initialize_account(contract, expr),
        ]
    }

    /// Create a new payment script.
    pub fn new_payment(from: &Pubkey, to: &Pubkey, lamports: u64) -> Vec<Instruction> {
        let contract = Keypair::new().pubkey();
        let expr = BudgetExpr::new_payment(lamports, to);
        Self::new_account(from, &contract, lamports, expr)
    }

    /// Create a future payment script.
    pub fn new_on_date(
        from: &Pubkey,
        to: &Pubkey,
        contract: &Pubkey,
        dt: DateTime<Utc>,
        dt_pubkey: &Pubkey,
        cancelable: Option<Pubkey>,
        lamports: u64,
    ) -> Vec<Instruction> {
        let expr =
            BudgetExpr::new_cancelable_future_payment(dt, dt_pubkey, lamports, to, cancelable);
        Self::new_account(from, contract, lamports, expr)
    }

    /// Create a multisig payment script.
    pub fn new_when_signed(
        from: &Pubkey,
        to: &Pubkey,
        contract: &Pubkey,
        witness: &Pubkey,
        cancelable: Option<Pubkey>,
        lamports: u64,
    ) -> Vec<Instruction> {
        let expr = BudgetExpr::new_cancelable_authorized_payment(witness, lamports, to, cancelable);
        Self::new_account(from, contract, lamports, expr)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget_expr::BudgetExpr;

    #[test]
    fn test_budget_instruction_verify() {
        let alice_pubkey = Keypair::new().pubkey();
        let bob_pubkey = Keypair::new().pubkey();
        BudgetInstruction::new_payment(&alice_pubkey, &bob_pubkey, 1); // No panic! indicates success.
    }

    #[test]
    #[should_panic]
    fn test_budget_instruction_overspend() {
        let alice_pubkey = Keypair::new().pubkey();
        let bob_pubkey = Keypair::new().pubkey();
        let budget_pubkey = Keypair::new().pubkey();
        let expr = BudgetExpr::new_payment(2, &bob_pubkey);
        BudgetInstruction::new_account(&alice_pubkey, &budget_pubkey, 1, expr);
    }

    #[test]
    #[should_panic]
    fn test_budget_instruction_underspend() {
        let alice_pubkey = Keypair::new().pubkey();
        let bob_pubkey = Keypair::new().pubkey();
        let budget_pubkey = Keypair::new().pubkey();
        let expr = BudgetExpr::new_payment(1, &bob_pubkey);
        BudgetInstruction::new_account(&alice_pubkey, &budget_pubkey, 2, expr);
    }
}
