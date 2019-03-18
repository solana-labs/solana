use crate::budget_expr::{BudgetExpr, Condition};
use crate::budget_instruction::BudgetInstruction;
use crate::budget_state::BudgetState;
use crate::id;
use bincode::serialized_size;
use chrono::prelude::{DateTime, Utc};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::script::Script;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;

pub struct BudgetScript {}

impl BudgetScript {
    pub fn new_account(
        from: &Pubkey,
        contract: &Pubkey,
        lamports: u64,
        expr: BudgetExpr,
    ) -> Script {
        let space = serialized_size(&BudgetState::new(expr.clone())).unwrap();
        let instructions = vec![
            SystemInstruction::new_program_account(&from, contract, lamports, space, &id()),
            BudgetInstruction::new_initialize_account(contract, expr),
        ];
        Script::new(instructions)
    }

    /// Create a new payment script.
    pub fn pay(from: &Pubkey, to: &Pubkey, lamports: u64) -> Script {
        let contract = Keypair::new().pubkey();
        let expr = BudgetExpr::new_payment(lamports, to);
        Self::new_account(from, &contract, lamports, expr)
    }

    /// Create a future payment script.
    pub fn pay_on_date(
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

        Self::new_account(from, contract, lamports, expr)
    }

    /// Create a multisig payment script.
    pub fn pay_on_signature(
        from: &Pubkey,
        to: &Pubkey,
        contract: &Pubkey,
        witness: &Pubkey,
        cancelable: Option<Pubkey>,
        lamports: u64,
    ) -> Script {
        let expr = if let Some(from) = cancelable {
            BudgetExpr::new_cancelable_authorized_payment(witness, lamports, to, &from)
        } else {
            BudgetExpr::new_authorized_payment(witness, lamports, to)
        };

        Self::new_account(from, contract, lamports, expr)
    }
}
