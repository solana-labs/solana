//! The `budget_transaction` module provides functionality for creating Budget transactions.

use bincode::{deserialize, serialize};
use budget::{Budget, Condition};
use budget_instruction::{Contract, Instruction, Vote};
use budget_program::BudgetState;
use chrono::prelude::*;
use hash::Hash;
use payment_plan::Payment;
use signature::{Keypair, Pubkey};
use transaction::Transaction;

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

    fn budget_new_vote(from_keypair: &Keypair, vote: Vote, last_id: Hash, fee: i64) -> Self;

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

    fn vote(&self) -> Option<(Pubkey, Vote, Hash)>;

    fn instruction(&self) -> Option<Instruction>;

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
        let payment = Payment {
            tokens: tokens - fee,
            to,
        };
        let budget = Budget::Pay(payment);
        let instruction = Instruction::NewContract(Contract { budget, tokens });
        let userdata = serialize(&instruction).unwrap();
        Self::new(
            from_keypair,
            &[to],
            BudgetState::id(),
            userdata,
            last_id,
            fee,
        )
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

    fn budget_new_vote(from_keypair: &Keypair, vote: Vote, last_id: Hash, fee: i64) -> Self {
        let instruction = Instruction::NewVote(vote);
        let userdata = serialize(&instruction).expect("serialize instruction");
        Self::new(from_keypair, &[], BudgetState::id(), userdata, last_id, fee)
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
        let budget = if let Some(from) = cancelable {
            Budget::Or(
                (Condition::Timestamp(dt, dt_pubkey), Payment { tokens, to }),
                (Condition::Signature(from), Payment { tokens, to: from }),
            )
        } else {
            Budget::After(Condition::Timestamp(dt, dt_pubkey), Payment { tokens, to })
        };
        let instruction = Instruction::NewContract(Contract { budget, tokens });
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
        let budget = if let Some(from) = cancelable {
            Budget::Or(
                (Condition::Signature(witness), Payment { tokens, to }),
                (Condition::Signature(from), Payment { tokens, to: from }),
            )
        } else {
            Budget::After(Condition::Signature(witness), Payment { tokens, to })
        };
        let instruction = Instruction::NewContract(Contract { budget, tokens });
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

    fn vote(&self) -> Option<(Pubkey, Vote, Hash)> {
        if let Some(Instruction::NewVote(vote)) = self.instruction() {
            Some((*self.from(), vote, self.last_id))
        } else {
            None
        }
    }

    fn instruction(&self) -> Option<Instruction> {
        deserialize(&self.userdata).ok()
    }

    /// Verify only the payment plan.
    fn verify_plan(&self) -> bool {
        if let Some(Instruction::NewContract(contract)) = self.instruction() {
            self.fee >= 0
                && self.fee <= contract.tokens
                && contract.budget.verify(contract.tokens - self.fee)
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize};
    use signature::KeypairUtil;

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
        let budget = Budget::Pay(Payment {
            tokens: 0,
            to: Default::default(),
        });
        let instruction = Instruction::NewContract(Contract { budget, tokens: 0 });
        let userdata = serialize(&instruction).unwrap();
        let claim0 = Transaction {
            keys: vec![],
            last_id: Default::default(),
            signature: Default::default(),
            program_id: Default::default(),
            fee: 0,
            userdata,
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
        let mut instruction = tx.instruction().unwrap();
        if let Instruction::NewContract(ref mut contract) = instruction {
            contract.tokens = 1_000_000; // <-- attack, part 1!
            if let Budget::Pay(ref mut payment) = contract.budget {
                payment.tokens = contract.tokens; // <-- attack, part 2!
            }
        }
        tx.userdata = serialize(&instruction).unwrap();
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
        let mut instruction = tx.instruction();
        if let Some(Instruction::NewContract(ref mut contract)) = instruction {
            if let Budget::Pay(ref mut payment) = contract.budget {
                payment.to = thief_keypair.pubkey(); // <-- attack!
            }
        }
        tx.userdata = serialize(&instruction).unwrap();
        assert!(tx.verify_plan());
        assert!(!tx.verify_signature());
    }

    #[test]
    fn test_overspend_attack() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let zero = Hash::default();
        let mut tx = Transaction::budget_new(&keypair0, keypair1.pubkey(), 1, zero);
        let mut instruction = tx.instruction().unwrap();
        if let Instruction::NewContract(ref mut contract) = instruction {
            if let Budget::Pay(ref mut payment) = contract.budget {
                payment.tokens = 2; // <-- attack!
            }
        }
        tx.userdata = serialize(&instruction).unwrap();
        assert!(!tx.verify_plan());

        // Also, ensure all branchs of the plan spend all tokens
        let mut instruction = tx.instruction().unwrap();
        if let Instruction::NewContract(ref mut contract) = instruction {
            if let Budget::Pay(ref mut payment) = contract.budget {
                payment.tokens = 0; // <-- whoops!
            }
        }
        tx.userdata = serialize(&instruction).unwrap();
        assert!(!tx.verify_plan());
    }
}
