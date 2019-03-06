//! The `budget_transaction` module provides functionality for creating Budget transactions.

use crate::budget_expr::{BudgetExpr, Condition};
use crate::budget_instruction::BudgetInstruction;
use crate::budget_state::BudgetState;
use crate::id;
use bincode::{deserialize, serialized_size};
use chrono::prelude::*;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::Transaction;
use solana_sdk::transaction_builder::TransactionBuilder;

pub struct BudgetTransaction {}

impl BudgetTransaction {
    /// Create and sign a new Transaction. Used for unit-testing.
    pub fn new_payment(
        from_keypair: &Keypair,
        to: Pubkey,
        lamports: u64,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let contract = Keypair::new().pubkey();
        let from = from_keypair.pubkey();
        let payment = BudgetExpr::new_payment(lamports - fee, to);
        let space = serialized_size(&BudgetState::new(payment.clone())).unwrap();
        TransactionBuilder::new(fee)
            .push(SystemInstruction::new_program_account(
                from,
                contract,
                lamports,
                space,
                id(),
            ))
            .push(BudgetInstruction::new_initialize_account(contract, payment))
            .sign(&[from_keypair], recent_blockhash)
    }

    /// Create and sign a new Transaction. Used for unit-testing.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        from_keypair: &Keypair,
        to: Pubkey,
        lamports: u64,
        recent_blockhash: Hash,
    ) -> Transaction {
        Self::new_payment(from_keypair, to, lamports, recent_blockhash, 0)
    }

    /// Create and sign a new Witness Timestamp. Used for unit-testing.
    pub fn new_timestamp(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        dt: DateTime<Utc>,
        recent_blockhash: Hash,
    ) -> Transaction {
        let instruction = BudgetInstruction::ApplyTimestamp(dt);
        Transaction::new(
            from_keypair,
            &[contract, to],
            id(),
            &instruction,
            recent_blockhash,
            0,
        )
    }

    /// Create and sign a new Witness Signature. Used for unit-testing.
    pub fn new_signature(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        recent_blockhash: Hash,
    ) -> Transaction {
        let instruction = BudgetInstruction::ApplySignature;
        let mut keys = vec![contract];
        if from_keypair.pubkey() != to {
            keys.push(to);
        }
        Transaction::new(from_keypair, &keys, id(), &instruction, recent_blockhash, 0)
    }

    /// Create and sign a postdated Transaction. Used for unit-testing.
    pub fn new_on_date(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        dt: DateTime<Utc>,
        dt_pubkey: Pubkey,
        cancelable: Option<Pubkey>,
        lamports: u64,
        recent_blockhash: Hash,
    ) -> Transaction {
        let expr = if let Some(from) = cancelable {
            BudgetExpr::Or(
                (
                    Condition::Timestamp(dt, dt_pubkey),
                    Box::new(BudgetExpr::new_payment(lamports, to)),
                ),
                (
                    Condition::Signature(from),
                    Box::new(BudgetExpr::new_payment(lamports, from)),
                ),
            )
        } else {
            BudgetExpr::After(
                Condition::Timestamp(dt, dt_pubkey),
                Box::new(BudgetExpr::new_payment(lamports, to)),
            )
        };
        let instruction = BudgetInstruction::InitializeAccount(expr);
        Transaction::new(
            from_keypair,
            &[contract],
            id(),
            &instruction,
            recent_blockhash,
            0,
        )
    }
    /// Create and sign a multisig Transaction.
    pub fn new_when_signed(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        witness: Pubkey,
        cancelable: Option<Pubkey>,
        lamports: u64,
        recent_blockhash: Hash,
    ) -> Transaction {
        let expr = if let Some(from) = cancelable {
            BudgetExpr::Or(
                (
                    Condition::Signature(witness),
                    Box::new(BudgetExpr::new_payment(lamports, to)),
                ),
                (
                    Condition::Signature(from),
                    Box::new(BudgetExpr::new_payment(lamports, from)),
                ),
            )
        } else {
            BudgetExpr::After(
                Condition::Signature(witness),
                Box::new(BudgetExpr::new_payment(lamports, to)),
            )
        };
        let instruction = BudgetInstruction::InitializeAccount(expr);
        Transaction::new(
            from_keypair,
            &[contract],
            id(),
            &instruction,
            recent_blockhash,
            0,
        )
    }

    pub fn system_instruction(tx: &Transaction, index: usize) -> Option<SystemInstruction> {
        deserialize(&tx.userdata(index)).ok()
    }

    pub fn instruction(tx: &Transaction, index: usize) -> Option<BudgetInstruction> {
        deserialize(&tx.userdata(index)).ok()
    }

    /// Verify only the payment plan.
    pub fn verify_plan(tx: &Transaction) -> bool {
        if let Some(SystemInstruction::CreateAccount { lamports, .. }) =
            Self::system_instruction(tx, 0)
        {
            if let Some(BudgetInstruction::InitializeAccount(expr)) =
                BudgetTransaction::instruction(&tx, 1)
            {
                if !(tx.fee <= lamports && expr.verify(lamports - tx.fee)) {
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
        let tx0 = BudgetTransaction::new(&keypair, keypair.pubkey(), 42, zero);
        assert!(BudgetTransaction::verify_plan(&tx0));
    }

    #[test]
    fn test_payment() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let tx0 = BudgetTransaction::new(&keypair0, pubkey1, 42, zero);
        assert!(BudgetTransaction::verify_plan(&tx0));
    }

    #[test]
    fn test_payment_with_fee() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let pubkey1 = Keypair::new().pubkey();
        let tx0 = BudgetTransaction::new_payment(&keypair0, pubkey1, 1, zero, 1);
        assert!(BudgetTransaction::verify_plan(&tx0));
    }

    #[test]
    fn test_serialize_claim() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let pubkey1 = Keypair::new().pubkey();
        let tx0 = BudgetTransaction::new_payment(&keypair0, pubkey1, 1, zero, 1);
        let buf = serialize(&tx0).unwrap();
        let tx1: Transaction = deserialize(&buf).unwrap();
        assert_eq!(tx1, tx0);
    }

    #[test]
    fn test_lamport_attack() {
        let zero = Hash::default();
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let mut tx = BudgetTransaction::new(&keypair, pubkey, 42, zero);
        let mut system_instruction = BudgetTransaction::system_instruction(&tx, 0).unwrap();
        if let SystemInstruction::CreateAccount {
            ref mut lamports, ..
        } = system_instruction
        {
            *lamports = 1_000_000; // <-- attack, part 1!
            let mut instruction = BudgetTransaction::instruction(&tx, 1).unwrap();
            if let BudgetInstruction::InitializeAccount(ref mut expr) = instruction {
                if let BudgetExpr::Pay(ref mut payment) = expr {
                    payment.lamports = *lamports; // <-- attack, part 2!
                }
            }
            tx.instructions[1].userdata = serialize(&instruction).unwrap();
        }
        tx.instructions[0].userdata = serialize(&system_instruction).unwrap();
        assert!(BudgetTransaction::verify_plan(&tx));
        assert!(!tx.verify_signature());
    }

    #[test]
    fn test_hijack_attack() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let thief_keypair = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let zero = Hash::default();
        let mut tx = BudgetTransaction::new(&keypair0, pubkey1, 42, zero);
        let mut instruction = BudgetTransaction::instruction(&tx, 1);
        if let Some(BudgetInstruction::InitializeAccount(ref mut expr)) = instruction {
            if let BudgetExpr::Pay(ref mut payment) = expr {
                payment.to = thief_keypair.pubkey(); // <-- attack!
            }
        }
        tx.instructions[1].userdata = serialize(&instruction).unwrap();
        assert!(BudgetTransaction::verify_plan(&tx));
        assert!(!tx.verify_signature());
    }

    #[test]
    fn test_overspend_attack() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let zero = Hash::default();
        let mut tx = BudgetTransaction::new(&keypair0, keypair1.pubkey(), 1, zero);
        let mut instruction = BudgetTransaction::instruction(&tx, 1).unwrap();
        if let BudgetInstruction::InitializeAccount(ref mut expr) = instruction {
            if let BudgetExpr::Pay(ref mut payment) = expr {
                payment.lamports = 2; // <-- attack!
            }
        }
        tx.instructions[1].userdata = serialize(&instruction).unwrap();
        assert!(!BudgetTransaction::verify_plan(&tx));

        // Also, ensure all branchs of the plan spend all lamports
        let mut instruction = BudgetTransaction::instruction(&tx, 1).unwrap();
        if let BudgetInstruction::InitializeAccount(ref mut expr) = instruction {
            if let BudgetExpr::Pay(ref mut payment) = expr {
                payment.lamports = 0; // <-- whoops!
            }
        }
        tx.instructions[1].userdata = serialize(&instruction).unwrap();
        assert!(!BudgetTransaction::verify_plan(&tx));
    }
}
