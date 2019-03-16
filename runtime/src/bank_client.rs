use crate::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::{Script, Transaction, TransactionError};

pub struct BankClient<'a> {
    bank: &'a Bank,
    keypair: Keypair,
}

impl<'a> BankClient<'a> {
    pub fn new(bank: &'a Bank, keypair: Keypair) -> Self {
        Self { bank, keypair }
    }

    pub fn pubkey(&self) -> Pubkey {
        self.keypair.pubkey()
    }

    pub fn process_transaction(&self, mut tx: Transaction) -> Result<(), TransactionError> {
        tx.sign(&[&self.keypair], self.bank.last_blockhash());
        self.bank.process_transaction(&mut tx)
    }

    /// Create and process a transaction.
    pub fn process_script(&self, script: Script) -> Result<(), TransactionError> {
        self.process_transaction(Transaction::new(script))
    }

    /// Transfer lamports to pubkey
    pub fn transfer(&self, lamports: u64, pubkey: &Pubkey) -> Result<(), TransactionError> {
        let move_instruction = SystemInstruction::new_move(&self.pubkey(), pubkey, lamports);
        self.process_script(vec![move_instruction])
    }
}
