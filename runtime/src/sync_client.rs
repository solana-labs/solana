use crate::bank::Bank;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::{Transaction, TransactionError};

// A client where each method waits for the server to complete before returning.
pub trait SyncClient {
    fn send_transaction(
        &self,
        keypairs: &[&Keypair],
        tx: Transaction,
    ) -> Result<(), TransactionError>;

    /// Create and process a transaction from a list of instructions.
    fn send_instructions(
        &self,
        keypairs: &[&Keypair],
        instructions: Vec<Instruction>,
    ) -> Result<(), TransactionError>;

    /// Create and process a transaction from a single instruction.
    fn send_instruction(
        &self,
        keypairs: &[&Keypair],
        instruction: Instruction,
    ) -> Result<(), TransactionError>;

    /// Transfer lamports to pubkey
    fn pay(
        &self,
        from_keypair: &Keypair,
        to_pubkey: &Pubkey,
        lamports: u64,
    ) -> Result<(), TransactionError>;
}

impl SyncClient for Bank {
    fn send_transaction(
        &self,
        keypairs: &[&Keypair],
        mut tx: Transaction,
    ) -> Result<(), TransactionError> {
        tx.sign(keypairs, self.last_blockhash());
        self.process_transaction(&tx)
    }

    /// Create and process a transaction from a list of instructions.
    fn send_instructions(
        &self,
        keypairs: &[&Keypair],
        instructions: Vec<Instruction>,
    ) -> Result<(), TransactionError> {
        self.send_transaction(keypairs, Transaction::new(instructions))
    }

    /// Create and process a transaction from a single instruction.
    fn send_instruction(
        &self,
        keypairs: &[&Keypair],
        instruction: Instruction,
    ) -> Result<(), TransactionError> {
        self.send_instructions(keypairs, vec![instruction])
    }

    /// Transfer lamports to pubkey
    fn pay(
        &self,
        from_keypair: &Keypair,
        to_pubkey: &Pubkey,
        lamports: u64,
    ) -> Result<(), TransactionError> {
        let move_instruction =
            SystemInstruction::new_move(&from_keypair.pubkey(), to_pubkey, lamports);
        self.send_instruction(&[from_keypair], move_instruction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::instruction::AccountMeta;

    #[test]
    fn test_sync_client_new_with_keypairs() {
        let (genesis_block, john_doe_keypair) = GenesisBlock::new(10_000);
        let john_pubkey = john_doe_keypair.pubkey();
        let jane_doe_keypair = Keypair::new();
        let jane_pubkey = jane_doe_keypair.pubkey();
        let doe_keypairs = vec![&john_doe_keypair, &jane_doe_keypair];
        let bank = Bank::new(&genesis_block);

        // Create 2-2 Multisig Move instruction.
        let bob_pubkey = Keypair::new().pubkey();
        let mut move_instruction = SystemInstruction::new_move(&john_pubkey, &bob_pubkey, 42);
        move_instruction
            .accounts
            .push(AccountMeta::new(jane_pubkey, true));

        bank.send_instruction(&doe_keypairs, move_instruction)
            .unwrap();
        assert_eq!(bank.get_balance(&bob_pubkey), 42);
    }
}
