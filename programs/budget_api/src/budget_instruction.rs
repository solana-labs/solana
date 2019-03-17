use crate::budget_expr::{BudgetExpr, Condition};
use crate::budget_state::BudgetState;
use crate::id;
use bincode::serialized_size;
use chrono::prelude::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::script::Script;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::Instruction;

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
            keys.push((payment.to, false));
        }
        keys.push((*contract, false));
        Instruction::new(id(), &BudgetInstruction::InitializeAccount(expr), keys)
    }

    pub fn new_initialize_account_script(
        from: &Pubkey,
        contract: &Pubkey,
        lamports: u64,
        expr: BudgetExpr,
    ) -> Script {
        let space = serialized_size(&BudgetState::new(expr.clone())).unwrap();
        Script::new(vec![
            SystemInstruction::new_program_account(&from, contract, lamports, space, &id()),
            Self::new_initialize_account(contract, expr),
        ])
    }

    /// Create a new payment script.
    pub fn new_payment_script(from: &Pubkey, to: &Pubkey, lamports: u64) -> Script {
        let contract = Keypair::new().pubkey();
        let expr = BudgetExpr::new_payment(lamports, to);
        Self::new_initialize_account_script(from, &contract, lamports, expr)
    }

    /// Create a postdated payment script.
    pub fn new_on_date_script(
        from: &Pubkey,
        to: &Pubkey,
        contract: &Pubkey,
        dt: DateTime<Utc>,
        dt_pubkey: &Pubkey,
        cancelable: Option<Pubkey>,
        lamports: u64,
    ) -> Script {
        let expr = if let Some(from) = cancelable {
            BudgetExpr::Or(
                (
                    Condition::Timestamp(dt, *dt_pubkey),
                    Box::new(BudgetExpr::new_payment(lamports, to)),
                ),
                (
                    Condition::Signature(from),
                    Box::new(BudgetExpr::new_payment(lamports, &from)),
                ),
            )
        } else {
            BudgetExpr::After(
                Condition::Timestamp(dt, *dt_pubkey),
                Box::new(BudgetExpr::new_payment(lamports, to)),
            )
        };

        Self::new_initialize_account_script(from, contract, lamports, expr)
    }

    /// Create a multisig payment script.
    pub fn new_when_signed_script(
        from: &Pubkey,
        to: &Pubkey,
        contract: &Pubkey,
        witness: &Pubkey,
        cancelable: Option<Pubkey>,
        lamports: u64,
    ) -> Script {
        let expr = if let Some(from) = cancelable {
            BudgetExpr::Or(
                (
                    Condition::Signature(*witness),
                    Box::new(BudgetExpr::new_payment(lamports, to)),
                ),
                (
                    Condition::Signature(from),
                    Box::new(BudgetExpr::new_payment(lamports, &from)),
                ),
            )
        } else {
            BudgetExpr::After(
                Condition::Signature(*witness),
                Box::new(BudgetExpr::new_payment(lamports, to)),
            )
        };

        Self::new_initialize_account_script(from, contract, lamports, expr)
    }

    pub fn new_apply_timestamp(
        from: &Pubkey,
        contract: &Pubkey,
        to: &Pubkey,
        dt: DateTime<Utc>,
    ) -> Instruction {
        let mut keys = vec![(*from, true), (*contract, false)];
        if from != to {
            keys.push((*to, false));
        }
        Instruction::new(id(), &BudgetInstruction::ApplyTimestamp(dt), keys)
    }

    pub fn new_apply_signature(from: &Pubkey, contract: &Pubkey, to: &Pubkey) -> Instruction {
        let mut keys = vec![(*from, true), (*contract, false)];
        if from != to {
            keys.push((*to, false));
        }
        Instruction::new(id(), &BudgetInstruction::ApplySignature, keys)
    }
}
