use crate::config_instruction::ConfigInstruction;
use crate::ConfigState;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;

pub struct ConfigTransaction {}

impl ConfigTransaction {
    /// Create a new, empty configuration account
    pub fn new_account<T: ConfigState>(
        from_keypair: &Keypair,
        config_account_pubkey: &Pubkey,
        recent_blockhash: Hash,
        lamports: u64,
        fee: u64,
    ) -> Transaction {
        let mut transaction = Transaction::new(vec![ConfigInstruction::new_account::<T>(
            &from_keypair.pubkey(),
            config_account_pubkey,
            lamports,
        )]);
        transaction.fee = fee;

        transaction.sign(&[from_keypair], recent_blockhash);
        transaction
    }

    /// Store new state in a configuration account
    pub fn new_store<T: ConfigState>(
        config_account_keypair: &Keypair,
        data: &T,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let mut transaction = Transaction::new(vec![ConfigInstruction::new_store(
            &config_account_keypair.pubkey(),
            data,
        )]);
        transaction.fee = fee;
        transaction.sign(&[config_account_keypair], recent_blockhash);
        transaction
    }
}
