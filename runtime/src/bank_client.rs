use crate::bank::Bank;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::{Transaction, TransactionError};

pub struct BankClient<'a> {
    bank: &'a Bank,
}

impl<'a> BankClient<'a> {
    pub fn new(bank: &'a Bank) -> Self {
        Self { bank }
    }

    pub fn process_message(
        &self,
        keypairs: &[&Keypair],
        message: Message,
    ) -> Result<(), TransactionError> {
        let blockhash = self.bank.last_blockhash();
        let transaction = Transaction::new(&keypairs, message, blockhash);
        self.bank.process_transaction(&transaction)
    }

    /// Create and process a transaction from a single instruction.
    pub fn process_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
    ) -> Result<(), TransactionError> {
        let message = Message::new(vec![instruction]);
        self.process_message(&[keypair], message)
    }

    /// Transfer `lamports` from `keypair` to `pubkey`
    pub fn transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
    ) -> Result<(), TransactionError> {
        let move_instruction = SystemInstruction::new_move(&keypair.pubkey(), pubkey, lamports);
        self.process_instruction(keypair, move_instruction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::instruction::AccountMeta;

    #[test]
    fn test_bank_client_new_with_keypairs() {
        let (genesis_block, john_doe_keypair) = GenesisBlock::new(10_000);
        let john_pubkey = john_doe_keypair.pubkey();
        let jane_doe_keypair = Keypair::new();
        let jane_pubkey = jane_doe_keypair.pubkey();
        let doe_keypairs = vec![&john_doe_keypair, &jane_doe_keypair];
        let bank = Bank::new(&genesis_block);
        let bank_client = BankClient::new(&bank);

        // Create 2-2 Multisig Move instruction.
        let bob_pubkey = Keypair::new().pubkey();
        let mut move_instruction = SystemInstruction::new_move(&john_pubkey, &bob_pubkey, 42);
        move_instruction
            .accounts
            .push(AccountMeta::new(jane_pubkey, true));

        let message = Message::new(vec![move_instruction]);
        bank_client.process_message(&doe_keypairs, message).unwrap();
        assert_eq!(bank.get_balance(&bob_pubkey), 42);
    }
}
