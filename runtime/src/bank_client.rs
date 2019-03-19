use crate::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::script::Script;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::{Instruction, Transaction, TransactionError};

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

    fn sign(&self, tx: &mut Transaction) {
        let keypairs: Vec<_> = self.keypairs.iter().collect();
        tx.sign(&keypairs, self.bank.last_blockhash());
    }

    pub fn process_transaction(&self, mut tx: Transaction) -> Result<(), TransactionError> {
        self.sign(&mut tx);
        self.bank.process_transaction(&tx)
    }

    /// Create and process a transaction.
    pub fn process_script(&self, script: Script) -> Result<(), TransactionError> {
        self.process_transaction(script.compile())
    }

    /// Create and process a transaction from a list of instructions.
    pub fn process_instructions(
        &self,
        instructions: Vec<Instruction>,
    ) -> Result<(), TransactionError> {
        self.process_script(Script::new(instructions))
    }

    /// Create and process a transaction from a single instruction.
    pub fn process_instruction(&self, instruction: Instruction) -> Result<(), TransactionError> {
        self.process_instructions(vec![instruction])
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
    use solana_sdk::transaction::AccountMeta;

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
            .push(AccountMeta(jane_pubkey, true));

        doe_client.process_instruction(move_instruction).unwrap();
        assert_eq!(bank.get_balance(&bob_pubkey), 42);
    }
}
