use crate::pubkey::Pubkey;
use crate::system_program;
use crate::transaction::{AccountMeta, Instruction};

#[derive(Serialize, Debug, Clone, PartialEq)]
pub enum SystemError {
    AccountAlreadyInUse,
    ResultWithNegativeLamports,
    SourceNotSystemAccount,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SystemInstruction {
    /// Create a new account
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - new account key
    /// * lamports - number of lamports to transfer to the new account
    /// * space - memory to allocate if greater then zero
    /// * program_id - the program id of the new account
    CreateAccount {
        lamports: u64,
        space: u64,
        program_id: Pubkey,
    },
    /// Assign account to a program
    /// * Transaction::keys[0] - account to assign
    Assign { program_id: Pubkey },
    /// Move lamports
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - destination
    Move { lamports: u64 },
}

impl SystemInstruction {
    pub fn new_program_account(
        from_id: &Pubkey,
        to_id: &Pubkey,
        lamports: u64,
        space: u64,
        program_id: &Pubkey,
    ) -> Instruction {
        Instruction::new(
            system_program::id(),
            &SystemInstruction::CreateAccount {
                lamports,
                space,
                program_id: *program_id,
            },
            vec![AccountMeta(*from_id, true), AccountMeta(*to_id, false)],
        )
    }

    pub fn new_move(from_id: &Pubkey, to_id: &Pubkey, lamports: u64) -> Instruction {
        Instruction::new(
            system_program::id(),
            &SystemInstruction::Move { lamports },
            vec![AccountMeta(*from_id, true), AccountMeta(*to_id, false)],
        )
    }
}
