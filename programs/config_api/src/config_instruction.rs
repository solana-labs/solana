use crate::id;
use crate::ConfigState;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::{AccountMeta, Instruction};

pub struct ConfigInstruction {}

impl ConfigInstruction {
    /// Create a new, empty configuration account
    pub fn new_account<T: ConfigState>(
        from_account_pubkey: &Pubkey,
        config_account_pubkey: &Pubkey,
        lamports: u64,
    ) -> Instruction {
        SystemInstruction::new_program_account(
            from_account_pubkey,
            config_account_pubkey,
            lamports,
            T::max_space(),
            &id(),
        )
    }

    /// Store new data in a configuration account
    pub fn new_store<T: ConfigState>(
        from_account_pubkey: &Pubkey,
        config_account_pubkey: &Pubkey,
        data: &T,
    ) -> Instruction {
        let account_metas = vec![
            AccountMeta(*from_account_pubkey, true),
            AccountMeta(*config_account_pubkey, true),
        ];
        Instruction::new(id(), data, account_metas)
    }
}
