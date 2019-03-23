//! The `budget_transaction` module provides functionality for creating Budget transactions.

use crate::budget_instruction::BudgetInstruction;
use chrono::prelude::*;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;

pub struct BudgetTransaction {}

impl BudgetTransaction {
    /// Create and sign a new Transaction. Used for unit-testing.
    pub fn new_payment(
        from_keypair: &Keypair,
        to: &Pubkey,
        lamports: u64,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let ixs = BudgetInstruction::new_payment(&from_keypair.pubkey(), to, lamports);
        Transaction::new_signed_instructions(&[from_keypair], ixs, recent_blockhash, fee)
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
        Transaction::new_signed_instructions(&[from_keypair], vec![ix], recent_blockhash, 0)
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
        Transaction::new_signed_instructions(&[from_keypair], vec![ix], recent_blockhash, 0)
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
        let ixs = BudgetInstruction::new_on_date(
            &from_keypair.pubkey(),
            to,
            contract,
            dt,
            dt_pubkey,
            cancelable,
            lamports,
        );
        Transaction::new_signed_instructions(&[from_keypair], ixs, recent_blockhash, 0)
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
        let ixs = BudgetInstruction::new_when_signed(
            &from_keypair.pubkey(),
            to,
            contract,
            witness,
            cancelable,
            lamports,
        );
        Transaction::new_signed_instructions(&[from_keypair], ixs, recent_blockhash, 0)
    }
}
