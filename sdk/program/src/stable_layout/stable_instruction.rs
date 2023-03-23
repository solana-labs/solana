//! `Instruction`, with a stable memory layout

use {
    crate::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        stable_layout::stable_vec::StableVec,
    },
    std::fmt::Debug,
};

/// `Instruction`, with a stable memory layout
///
/// This is used within the runtime to ensure memory mapping and memory accesses are valid.  We
/// rely on known addresses and offsets within the runtime, and since `Instruction`'s layout is
/// allowed to change, we must provide a way to lock down the memory layout.  `StableInstruction`
/// reimplements the bare minimum of `Instruction`'s API sufficient only for the runtime's needs.
///
/// # Examples
///
/// Creating a `StableInstruction` from an `Instruction`
///
/// ```
/// # use solana_program::{instruction::Instruction, pubkey::Pubkey, stable_layout::stable_instruction::StableInstruction};
/// # let program_id = Pubkey::default();
/// # let accounts = Vec::default();
/// # let data = Vec::default();
/// let instruction = Instruction { program_id, accounts, data };
/// let instruction = StableInstruction::from(instruction);
/// ```
#[derive(Debug, PartialEq)]
#[repr(C)]
pub struct StableInstruction {
    pub accounts: StableVec<AccountMeta>,
    pub data: StableVec<u8>,
    pub program_id: Pubkey,
}

impl From<Instruction> for StableInstruction {
    fn from(other: Instruction) -> Self {
        Self {
            accounts: other.accounts.into(),
            data: other.data.into(),
            program_id: other.program_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        memoffset::offset_of,
        std::mem::{align_of, size_of},
    };

    #[test]
    fn test_memory_layout() {
        assert_eq!(offset_of!(StableInstruction, accounts), 0);
        assert_eq!(offset_of!(StableInstruction, data), 24);
        assert_eq!(offset_of!(StableInstruction, program_id), 48);
        assert_eq!(align_of::<StableInstruction>(), 8);
        assert_eq!(size_of::<StableInstruction>(), 24 + 24 + 32);

        let program_id = Pubkey::new_unique();
        let account_meta1 = AccountMeta {
            pubkey: Pubkey::new_unique(),
            is_signer: true,
            is_writable: false,
        };
        let account_meta2 = AccountMeta {
            pubkey: Pubkey::new_unique(),
            is_signer: false,
            is_writable: true,
        };
        let accounts = vec![account_meta1, account_meta2];
        let data = vec![1, 2, 3, 4, 5];
        let instruction = Instruction {
            program_id,
            accounts: accounts.clone(),
            data: data.clone(),
        };
        let instruction = StableInstruction::from(instruction);

        let instruction_addr = &instruction as *const _ as u64;

        let accounts_ptr = instruction_addr as *const StableVec<AccountMeta>;
        assert_eq!(unsafe { &*accounts_ptr }, &accounts);

        let data_ptr = (instruction_addr + 24) as *const StableVec<u8>;
        assert_eq!(unsafe { &*data_ptr }, &data);

        let pubkey_ptr = (instruction_addr + 48) as *const Pubkey;
        assert_eq!(unsafe { *pubkey_ptr }, program_id);
    }
}
