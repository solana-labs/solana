//! The `budget_transaction` module provides functionality for creating Budget transactions.

use crate::budget_expr::{BudgetExpr, Condition};
use crate::budget_instruction::Instruction;
use crate::budget_program;
use crate::hash::Hash;
use crate::payment_plan::Payment;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil};
use crate::system_instruction::SystemInstruction;
use crate::system_program;
use crate::transaction::{self, Transaction};
use bincode::deserialize;
use chrono::prelude::*;

pub trait BudgetTransaction {
    fn budget_new_taxed(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: u64,
        fee: u64,
        last_id: Hash,
    ) -> Self;

    fn budget_new(from_keypair: &Keypair, to: Pubkey, tokens: u64, last_id: Hash) -> Self;

    fn budget_new_timestamp(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        dt: DateTime<Utc>,
        last_id: Hash,
    ) -> Self;

    fn budget_new_signature(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        last_id: Hash,
    ) -> Self;

    fn budget_new_on_date(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        dt: DateTime<Utc>,
        dt_pubkey: Pubkey,
        cancelable: Option<Pubkey>,
        tokens: u64,
        last_id: Hash,
    ) -> Self;

    fn budget_new_when_signed(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        witness: Pubkey,
        cancelable: Option<Pubkey>,
        tokens: u64,
        last_id: Hash,
    ) -> Self;

    fn instruction(&self, program_index: usize) -> Option<Instruction>;
    fn system_instruction(&self, program_index: usize) -> Option<SystemInstruction>;

    fn verify_plan(&self) -> bool;
}

impl BudgetTransaction for Transaction {
    /// Create and sign a new Transaction. Used for unit-testing.
    fn budget_new_taxed(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: u64,
        fee: u64,
        last_id: Hash,
    ) -> Self {
        let contract = Keypair::new().pubkey();
        let keys = vec![from_keypair.pubkey(), contract];

        let system_instruction = SystemInstruction::Move { tokens };

        let payment = Payment {
            tokens: tokens - fee,
            to,
        };
        let budget_instruction = Instruction::NewBudget(BudgetExpr::Pay(payment));

        let program_ids = vec![system_program::id(), budget_program::id()];

        let instructions = vec![
            transaction::Instruction::new(0, &system_instruction, vec![0, 1]),
            transaction::Instruction::new(1, &budget_instruction, vec![1]),
        ];

        Self::new_with_instructions(
            &[from_keypair],
            &keys,
            last_id,
            fee,
            program_ids,
            instructions,
        )
    }

    /// Create and sign a new Transaction. Used for unit-testing.
    fn budget_new(from_keypair: &Keypair, to: Pubkey, tokens: u64, last_id: Hash) -> Self {
        Self::budget_new_taxed(from_keypair, to, tokens, 0, last_id)
    }

    /// Create and sign a new Witness Timestamp. Used for unit-testing.
    fn budget_new_timestamp(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        dt: DateTime<Utc>,
        last_id: Hash,
    ) -> Self {
        let instruction = Instruction::ApplyTimestamp(dt);
        Self::new(
            from_keypair,
            &[contract, to],
            budget_program::id(),
            &instruction,
            last_id,
            0,
        )
    }

    /// Create and sign a new Witness Signature. Used for unit-testing.
    fn budget_new_signature(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        last_id: Hash,
    ) -> Self {
        let instruction = Instruction::ApplySignature;
        Self::new(
            from_keypair,
            &[contract, to],
            budget_program::id(),
            &instruction,
            last_id,
            0,
        )
    }

    /// Create and sign a postdated Transaction. Used for unit-testing.
    fn budget_new_on_date(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        dt: DateTime<Utc>,
        dt_pubkey: Pubkey,
        cancelable: Option<Pubkey>,
        tokens: u64,
        last_id: Hash,
    ) -> Self {
        let expr = if let Some(from) = cancelable {
            BudgetExpr::Or(
                (
                    Condition::Timestamp(dt, dt_pubkey),
                    Box::new(BudgetExpr::new_payment(tokens, to)),
                ),
                (
                    Condition::Signature(from),
                    Box::new(BudgetExpr::new_payment(tokens, from)),
                ),
            )
        } else {
            BudgetExpr::After(
                Condition::Timestamp(dt, dt_pubkey),
                Box::new(BudgetExpr::new_payment(tokens, to)),
            )
        };
        let instruction = Instruction::NewBudget(expr);
        Self::new(
            from_keypair,
            &[contract],
            budget_program::id(),
            &instruction,
            last_id,
            0,
        )
    }
    /// Create and sign a multisig Transaction.
    fn budget_new_when_signed(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        witness: Pubkey,
        cancelable: Option<Pubkey>,
        tokens: u64,
        last_id: Hash,
    ) -> Self {
        let expr = if let Some(from) = cancelable {
            BudgetExpr::Or(
                (
                    Condition::Signature(witness),
                    Box::new(BudgetExpr::new_payment(tokens, to)),
                ),
                (
                    Condition::Signature(from),
                    Box::new(BudgetExpr::new_payment(tokens, from)),
                ),
            )
        } else {
            BudgetExpr::After(
                Condition::Signature(witness),
                Box::new(BudgetExpr::new_payment(tokens, to)),
            )
        };
        let instruction = Instruction::NewBudget(expr);
        Self::new(
            from_keypair,
            &[contract],
            budget_program::id(),
            &instruction,
            last_id,
            0,
        )
    }

    fn instruction(&self, instruction_index: usize) -> Option<Instruction> {
        deserialize(&self.userdata(instruction_index)).ok()
    }

    fn system_instruction(&self, instruction_index: usize) -> Option<SystemInstruction> {
        deserialize(&self.userdata(instruction_index)).ok()
    }

    /// Verify only the payment plan.
    fn verify_plan(&self) -> bool {
        if let Some(SystemInstruction::Move { tokens }) = self.system_instruction(0) {
            if let Some(Instruction::NewBudget(expr)) = self.instruction(1) {
                if !(self.fee <= tokens && expr.verify(tokens - self.fee)) {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize};

    #[test]
    fn test_claim() {
        let keypair = Keypair::new();
        let zero = Hash::default();
        let tx0 = Transaction::budget_new(&keypair, keypair.pubkey(), 42, zero);
        assert!(tx0.verify_plan());
    }

    #[test]
    fn test_transfer() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let tx0 = Transaction::budget_new(&keypair0, pubkey1, 42, zero);
        assert!(tx0.verify_plan());
    }

    #[test]
    fn test_transfer_with_fee() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let pubkey1 = Keypair::new().pubkey();
        assert!(Transaction::budget_new_taxed(&keypair0, pubkey1, 1, 1, zero).verify_plan());
    }

    #[test]
    fn test_serialize_claim() {
        let expr = BudgetExpr::Pay(Payment {
            tokens: 0,
            to: Default::default(),
        });
        let instruction = Instruction::NewBudget(expr);
        let instructions = vec![transaction::Instruction::new(0, &instruction, vec![])];
        let claim0 = Transaction {
            account_keys: vec![],
            last_id: Default::default(),
            signatures: vec![],
            program_ids: vec![],
            instructions,
            fee: 0,
        };
        let buf = serialize(&claim0).unwrap();
        let claim1: Transaction = deserialize(&buf).unwrap();
        assert_eq!(claim1, claim0);
    }

    #[test]
    fn test_token_attack() {
        let zero = Hash::default();
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let mut tx = Transaction::budget_new(&keypair, pubkey, 42, zero);
        let mut system_instruction = tx.system_instruction(0).unwrap();
        if let SystemInstruction::Move { ref mut tokens } = system_instruction {
            *tokens = 1_000_000; // <-- attack, part 1!
            let mut instruction = tx.instruction(1).unwrap();
            if let Instruction::NewBudget(ref mut expr) = instruction {
                if let BudgetExpr::Pay(ref mut payment) = expr {
                    payment.tokens = *tokens; // <-- attack, part 2!
                }
            }
            tx.instructions[1].userdata = serialize(&instruction).unwrap();
        }
        tx.instructions[0].userdata = serialize(&system_instruction).unwrap();
        assert!(tx.verify_plan());
        assert!(!tx.verify_signature());
    }

    #[test]
    fn test_hijack_attack() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let thief_keypair = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let zero = Hash::default();
        let mut tx = Transaction::budget_new(&keypair0, pubkey1, 42, zero);
        let mut instruction = tx.instruction(1);
        if let Some(Instruction::NewBudget(ref mut expr)) = instruction {
            if let BudgetExpr::Pay(ref mut payment) = expr {
                payment.to = thief_keypair.pubkey(); // <-- attack!
            }
        }
        tx.instructions[1].userdata = serialize(&instruction).unwrap();
        assert!(tx.verify_plan());
        assert!(!tx.verify_signature());
    }

    #[test]
    fn test_overspend_attack() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let zero = Hash::default();
        let mut tx = Transaction::budget_new(&keypair0, keypair1.pubkey(), 1, zero);
        let mut instruction = tx.instruction(1).unwrap();
        if let Instruction::NewBudget(ref mut expr) = instruction {
            if let BudgetExpr::Pay(ref mut payment) = expr {
                payment.tokens = 2; // <-- attack!
            }
        }
        tx.instructions[1].userdata = serialize(&instruction).unwrap();
        assert!(!tx.verify_plan());

        // Also, ensure all branchs of the plan spend all tokens
        let mut instruction = tx.instruction(1).unwrap();
        if let Instruction::NewBudget(ref mut expr) = instruction {
            if let BudgetExpr::Pay(ref mut payment) = expr {
                payment.tokens = 0; // <-- whoops!
            }
        }
        tx.instructions[1].userdata = serialize(&instruction).unwrap();
        assert!(!tx.verify_plan());
    }
}
