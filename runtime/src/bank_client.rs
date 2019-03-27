use crate::bank::Bank;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::{Transaction, TransactionError};

pub struct BankClient<'a> {
    bank: &'a Bank,
    keypairs: Vec<Keypair>,
}

impl<'a> BankClient<'a> {
    pub fn new_with_keypairs(bank: &'a Bank, keypairs: Vec<Keypair>) -> Self {
        assert!(!keypairs.is_empty());
        Self { bank, keypairs }
    }

    pub fn new(bank: &'a Bank, keypair: Keypair) -> Self {
        Self::new_with_keypairs(bank, vec![keypair])
    }

    pub fn pubkey(&self) -> Pubkey {
        self.keypairs[0].pubkey()
    }

    pub fn pubkeys(&self) -> Vec<Pubkey> {
        self.keypairs.iter().map(|x| x.pubkey()).collect()
    }

    pub fn process_message(&self, message: Message) -> Result<(), TransactionError> {
        let keypairs: Vec<_> = self.keypairs.iter().collect();
        let blockhash = self.bank.last_blockhash();
        let transaction = Transaction::new(&keypairs, message, blockhash);
        self.bank.process_transaction(&transaction)
    }

    /// Create and process a transaction from a single instruction.
    pub fn process_instruction(&self, instruction: Instruction) -> Result<(), TransactionError> {
        let message = Message::new(vec![instruction]);
        self.process_message(message)
    }

    /// Transfer lamports to pubkey
    pub fn transfer(&self, lamports: u64, pubkey: &Pubkey) -> Result<(), TransactionError> {
        let move_instruction = SystemInstruction::new_move(&self.pubkey(), pubkey, lamports);
        self.process_instruction(move_instruction)
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
        let jane_doe_keypair = Keypair::new();
        let doe_keypairs = vec![john_doe_keypair, jane_doe_keypair];
        let bank = Bank::new(&genesis_block);
        let doe_client = BankClient::new_with_keypairs(&bank, doe_keypairs);
        let jane_pubkey = doe_client.pubkeys()[1];

        // Create 2-2 Multisig Move instruction.
        let bob_pubkey = Keypair::new().pubkey();
        let mut move_instruction =
            SystemInstruction::new_move(&doe_client.pubkey(), &bob_pubkey, 42);
        move_instruction
            .accounts
            .push(AccountMeta::new(jane_pubkey, true));

        doe_client.process_instruction(move_instruction).unwrap();
        assert_eq!(bank.get_balance(&bob_pubkey), 42);
    }
}
