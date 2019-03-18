//! The `budget_transaction` module provides functionality for creating Budget transactions.

use crate::budget_instruction::BudgetInstruction;
use crate::budget_script::BudgetScript;
use chrono::prelude::*;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::script::Script;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;

pub struct BudgetTransaction {}

impl BudgetTransaction {
    fn new_signed(
        from_keypair: &Keypair,
        script: Script,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let mut tx = script.compile();
        tx.fee = fee;
        tx.sign(&[from_keypair], recent_blockhash);
        tx
    }

    /// Create and sign a new Transaction. Used for unit-testing.
    pub fn new_payment(
        from_keypair: &Keypair,
        to: &Pubkey,
        lamports: u64,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let script = BudgetScript::pay(&from_keypair.pubkey(), to, lamports);
        Self::new_signed(from_keypair, script, recent_blockhash, fee)
    }

    /// Create and sign a new Witness Timestamp. Used for unit-testing.
    pub fn new_timestamp(
        from_keypair: &Keypair,
        contract: &Pubkey,
        to: &Pubkey,
        dt: DateTime<Utc>,
        recent_blockhash: Hash,
    ) -> Transaction {
        let from = from_keypair.pubkey();
        let ix = BudgetInstruction::new_apply_timestamp(&from, contract, to, dt);
        let mut tx = Transaction::new(vec![ix]);
        tx.sign(&[from_keypair], recent_blockhash);
        tx
    }

    /// Create and sign a new Witness Signature. Used for unit-testing.
    pub fn new_signature(
        from_keypair: &Keypair,
        contract: &Pubkey,
        to: &Pubkey,
        recent_blockhash: Hash,
    ) -> Transaction {
        let from = from_keypair.pubkey();
        let ix = BudgetInstruction::new_apply_signature(&from, contract, to);
        let mut tx = Transaction::new(vec![ix]);
        tx.sign(&[from_keypair], recent_blockhash);
        tx
    }

    /// Create and sign a postdated Transaction. Used for unit-testing.
    pub fn new_on_date(
        from_keypair: &Keypair,
        to: &Pubkey,
        contract: &Pubkey,
        dt: DateTime<Utc>,
        dt_pubkey: &Pubkey,
        cancelable: Option<Pubkey>,
        lamports: u64,
        recent_blockhash: Hash,
    ) -> Transaction {
        let script = BudgetScript::pay_on_date(
            &from_keypair.pubkey(),
            to,
            contract,
            dt,
            dt_pubkey,
            cancelable,
            lamports,
        );
        Self::new_signed(from_keypair, script, recent_blockhash, 0)
    }

    /// Create and sign a multisig Transaction.
    pub fn new_when_signed(
        from_keypair: &Keypair,
        to: &Pubkey,
        contract: &Pubkey,
        witness: &Pubkey,
        cancelable: Option<Pubkey>,
        lamports: u64,
        recent_blockhash: Hash,
    ) -> Transaction {
        let script = BudgetScript::pay_on_signature(
            &from_keypair.pubkey(),
            to,
            contract,
            witness,
            cancelable,
            lamports,
        );
        Self::new_signed(from_keypair, script, recent_blockhash, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget_expr::BudgetExpr;
    use bincode::{deserialize, serialize};
    use solana_sdk::system_instruction::SystemInstruction;

    fn deserialize_system_instruction(tx: &Transaction, index: usize) -> Option<SystemInstruction> {
        deserialize(&tx.data(index)).ok()
    }

    fn deserialize_budget_instruction(tx: &Transaction, index: usize) -> Option<BudgetInstruction> {
        deserialize(&tx.data(index)).ok()
    }

    /// Verify only the payment plan.
    fn verify_plan(tx: &Transaction) -> bool {
        if let Some(SystemInstruction::CreateAccount { lamports, .. }) =
            deserialize_system_instruction(tx, 0)
        {
            if let Some(BudgetInstruction::InitializeAccount(expr)) =
                deserialize_budget_instruction(&tx, 1)
            {
                if !expr.verify(lamports) {
                    return false;
                }
            }
        }
        true
    }

    #[test]
    fn test_claim() {
        let keypair = Keypair::new();
        let zero = Hash::default();
        let tx0 = BudgetTransaction::new_payment(&keypair, &keypair.pubkey(), 42, zero, 0);
        assert!(verify_plan(&tx0));
    }

    #[test]
    fn test_payment() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let tx0 = BudgetTransaction::new_payment(&keypair0, &pubkey1, 42, zero, 0);
        assert!(verify_plan(&tx0));
    }

    #[test]
    fn test_payment_with_fee() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let pubkey1 = Keypair::new().pubkey();
        let tx0 = BudgetTransaction::new_payment(&keypair0, &pubkey1, 1, zero, 1);
        assert!(verify_plan(&tx0));
    }

    #[test]
    fn test_serialize_claim() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let pubkey1 = Keypair::new().pubkey();
        let tx0 = BudgetTransaction::new_payment(&keypair0, &pubkey1, 1, zero, 1);
        let buf = serialize(&tx0).unwrap();
        let tx1: Transaction = deserialize(&buf).unwrap();
        assert_eq!(tx1, tx0);
    }

    #[test]
    fn test_lamport_attack() {
        let zero = Hash::default();
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let mut tx = BudgetTransaction::new_payment(&keypair, &pubkey, 42, zero, 0);
        let mut system_instruction = deserialize_system_instruction(&tx, 0).unwrap();
        if let SystemInstruction::CreateAccount {
            ref mut lamports, ..
        } = system_instruction
        {
            *lamports = 1_000_000; // <-- attack, part 1!
            let mut instruction = deserialize_budget_instruction(&tx, 1).unwrap();
            if let BudgetInstruction::InitializeAccount(ref mut expr) = instruction {
                if let BudgetExpr::Pay(ref mut payment) = expr {
                    payment.lamports = *lamports; // <-- attack, part 2!
                }
            }
            tx.instructions[1].data = serialize(&instruction).unwrap();
        }
        tx.instructions[0].data = serialize(&system_instruction).unwrap();
        assert!(verify_plan(&tx));
        assert!(!tx.verify_signature());
    }

    #[test]
    fn test_hijack_attack() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let thief_keypair = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let zero = Hash::default();
        let mut tx = BudgetTransaction::new_payment(&keypair0, &pubkey1, 42, zero, 0);
        let mut instruction = deserialize_budget_instruction(&tx, 1);
        if let Some(BudgetInstruction::InitializeAccount(ref mut expr)) = instruction {
            if let BudgetExpr::Pay(ref mut payment) = expr {
                payment.to = thief_keypair.pubkey(); // <-- attack!
            }
        }
        tx.instructions[1].data = serialize(&instruction).unwrap();
        assert!(verify_plan(&tx));
        assert!(!tx.verify_signature());
    }

    #[test]
    fn test_overspend_attack() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let zero = Hash::default();
        let mut tx = BudgetTransaction::new_payment(&keypair0, &keypair1.pubkey(), 1, zero, 0);
        let mut instruction = deserialize_budget_instruction(&tx, 1).unwrap();
        if let BudgetInstruction::InitializeAccount(ref mut expr) = instruction {
            if let BudgetExpr::Pay(ref mut payment) = expr {
                payment.lamports = 2; // <-- attack!
            }
        }
        tx.instructions[1].data = serialize(&instruction).unwrap();
        assert!(!verify_plan(&tx));

        // Also, ensure all branchs of the plan spend all lamports
        let mut instruction = deserialize_budget_instruction(&tx, 1).unwrap();
        if let BudgetInstruction::InitializeAccount(ref mut expr) = instruction {
            if let BudgetExpr::Pay(ref mut payment) = expr {
                payment.lamports = 0; // <-- whoops!
            }
        }
        tx.instructions[1].data = serialize(&instruction).unwrap();
        assert!(!verify_plan(&tx));
    }
}
