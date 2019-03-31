use crate::instruction::{AccountMeta, Instruction};
use crate::pubkey::Pubkey;
use crate::system_program;

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
        let account_metas = vec![
            AccountMeta::new(*from_id, true),
            AccountMeta::new(*to_id, false),
        ];
        Instruction::new(
            system_program::id(),
            &SystemInstruction::CreateAccount {
                lamports,
                space,
                program_id: *program_id,
            },
            account_metas,
        )
    }

    /// Create and sign a transaction to create a system account
    pub fn new_account(from_id: &Pubkey, to_id: &Pubkey, lamports: u64) -> Instruction {
        let program_id = system_program::id();
        Self::new_program_account(from_id, to_id, lamports, 0, &program_id)
    }

    pub fn new_assign(from_id: &Pubkey, program_id: &Pubkey) -> Instruction {
        let account_metas = vec![AccountMeta::new(*from_id, true)];
        Instruction::new(
            system_program::id(),
            &SystemInstruction::Assign {
                program_id: *program_id,
            },
            account_metas,
        )
    }

    pub fn new_move(from_id: &Pubkey, to_id: &Pubkey, lamports: u64) -> Instruction {
        let account_metas = vec![
            AccountMeta::new(*from_id, true),
            AccountMeta::new(*to_id, false),
        ];
        Instruction::new(
            system_program::id(),
            &SystemInstruction::Move { lamports },
            account_metas,
        )
    }

    /// Create and sign new SystemInstruction::Move transaction to many destinations
    pub fn new_move_many(from_id: &Pubkey, to_lamports: &[(Pubkey, u64)]) -> Vec<Instruction> {
        to_lamports
            .iter()
            .map(|(to_id, lamports)| SystemInstruction::new_move(from_id, to_id, *lamports))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_keys(instruction: &Instruction) -> Vec<Pubkey> {
        instruction.accounts.iter().map(|x| x.pubkey).collect()
    }

    #[test]
    fn test_move_many() {
        let alice_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let carol_pubkey = Pubkey::new_rand();
        let to_lamports = vec![(bob_pubkey, 1), (carol_pubkey, 2)];

        let instructions = SystemInstruction::new_move_many(&alice_pubkey, &to_lamports);
        assert_eq!(instructions.len(), 2);
        assert_eq!(get_keys(&instructions[0]), vec![alice_pubkey, bob_pubkey]);
        assert_eq!(get_keys(&instructions[1]), vec![alice_pubkey, carol_pubkey]);
    }
}
