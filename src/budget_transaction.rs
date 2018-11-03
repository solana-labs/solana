//! The `budget_transaction` module provides functionality for creating Budget transactions.

use bincode::{deserialize, serialize};
use budget_expr::{BudgetExpr, Condition};
use budget_instruction::Instruction;
use budget_program::BudgetState;
use chrono::prelude::*;
use hash::Hash;
use payment_plan::Payment;
use signature::{Keypair, KeypairUtil};
use solana_sdk::pubkey::Pubkey;
use system_program::SystemProgram;
use transaction::{self, Transaction};

pub trait BudgetTransaction {
    fn budget_new_taxed(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: i64,
        fee: i64,
        last_id: Hash,
    ) -> Self;

    fn budget_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self;

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
        tokens: i64,
        last_id: Hash,
    ) -> Self;

    fn budget_new_when_signed(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        witness: Pubkey,
        cancelable: Option<Pubkey>,
        tokens: i64,
        last_id: Hash,
    ) -> Self;

    fn instruction(&self, program_index: usize) -> Option<Instruction>;
    fn system_instruction(&self, program_index: usize) -> Option<SystemProgram>;

    fn verify_plan(&self) -> bool;
}

impl BudgetTransaction for Transaction {
    /// Create and sign a new Transaction. Used for unit-testing.
    fn budget_new_taxed(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: i64,
        fee: i64,
        last_id: Hash,
    ) -> Self {
        let contract = Keypair::new().pubkey();
        let keys = vec![from_keypair.pubkey(), contract];

        let system_instruction = SystemProgram::Move { tokens };
        let move_userdata = serialize(&system_instruction).unwrap();

        let payment = Payment {
            tokens: tokens - fee,
            to,
        };
        let budget_instruction = Instruction::NewBudget(BudgetExpr::Pay(payment));
        let pay_userdata = serialize(&budget_instruction).unwrap();

        let program_ids = vec![SystemProgram::id(), BudgetState::id()];

        let instructions = vec![
            transaction::Instruction {
                program_ids_index: 0,
                userdata: move_userdata,
                accounts: vec![0, 1],
            },
            transaction::Instruction {
                program_ids_index: 1,
                userdata: pay_userdata,
                accounts: vec![1],
            },
        ];

        Self::new_with_instructions(from_keypair, &keys, last_id, fee, program_ids, instructions)
    }

    /// Create and sign a new Transaction. Used for unit-testing.
    fn budget_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self {
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
        let userdata = serialize(&instruction).unwrap();
        Self::new(
            from_keypair,
            &[contract, to],
            BudgetState::id(),
            userdata,
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
        let userdata = serialize(&instruction).unwrap();
        Self::new(
            from_keypair,
            &[contract, to],
            BudgetState::id(),
            userdata,
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
        tokens: i64,
        last_id: Hash,
    ) -> Self {
        let expr = if let Some(from) = cancelable {
            BudgetExpr::Or(
                (Condition::Timestamp(dt, dt_pubkey), Payment { tokens, to }),
                (Condition::Signature(from), Payment { tokens, to: from }),
            )
        } else {
            BudgetExpr::After(Condition::Timestamp(dt, dt_pubkey), Payment { tokens, to })
        };
        let instruction = Instruction::NewBudget(expr);
        let userdata = serialize(&instruction).expect("serialize instruction");
        Self::new(
            from_keypair,
            &[contract],
            BudgetState::id(),
            userdata,
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
        tokens: i64,
        last_id: Hash,
    ) -> Self {
        let expr = if let Some(from) = cancelable {
            BudgetExpr::Or(
                (Condition::Signature(witness), Payment { tokens, to }),
                (Condition::Signature(from), Payment { tokens, to: from }),
            )
        } else {
            BudgetExpr::After(Condition::Signature(witness), Payment { tokens, to })
        };
        let instruction = Instruction::NewBudget(expr);
        let userdata = serialize(&instruction).expect("serialize instruction");
        Self::new(
            from_keypair,
            &[contract],
            BudgetState::id(),
            userdata,
            last_id,
            0,
        )
    }

    fn instruction(&self, instruction_index: usize) -> Option<Instruction> {
        deserialize(&self.userdata(instruction_index)).ok()
    }

    fn system_instruction(&self, instruction_index: usize) -> Option<SystemProgram> {
        deserialize(&self.userdata(instruction_index)).ok()
    }

    /// Verify only the payment plan.
    fn verify_plan(&self) -> bool {
        if let Some(SystemProgram::Move { tokens }) = self.system_instruction(0) {
            if let Some(Instruction::NewBudget(expr)) = self.instruction(1) {
                if !(self.fee >= 0 && self.fee <= tokens && expr.verify(tokens - self.fee)) {
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
    use signature::KeypairUtil;
    use transaction;

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
        assert!(!Transaction::budget_new_taxed(&keypair0, pubkey1, 1, 2, zero).verify_plan());
        assert!(!Transaction::budget_new_taxed(&keypair0, pubkey1, 1, -1, zero).verify_plan());
    }

    #[test]
    fn test_serialize_claim() {
        let expr = BudgetExpr::Pay(Payment {
            tokens: 0,
            to: Default::default(),
        });
        let instruction = Instruction::NewBudget(expr);
        let userdata = serialize(&instruction).unwrap();
        let instructions = vec![transaction::Instruction {
            program_ids_index: 0,
            userdata,
            accounts: vec![],
        }];
        let claim0 = Transaction {
            account_keys: vec![],
            last_id: Default::default(),
            signature: Default::default(),
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
        if let SystemProgram::Move { ref mut tokens } = system_instruction {
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
