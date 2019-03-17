use crate::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::script::Script;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::{Instruction, Transaction, TransactionError};

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
