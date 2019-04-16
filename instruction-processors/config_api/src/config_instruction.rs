use crate::id;
use crate::ConfigState;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction;

/// Create a new, empty configuration account
pub fn create_account<T: ConfigState>(
    from_account_pubkey: &Pubkey,
    config_account_pubkey: &Pubkey,
    lamports: u64,
) -> Instruction {
    system_instruction::create_account(
        from_account_pubkey,
        config_account_pubkey,
        lamports,
        T::max_space(),
        &id(),
    )
}

/// Store new data in a configuration account
pub fn store<T: ConfigState>(
    from_account_pubkey: &Pubkey,
    config_account_pubkey: &Pubkey,
    data: &T,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_account_pubkey, true),
        AccountMeta::new(*config_account_pubkey, true),
    ];
    Instruction::new(id(), data, account_metas)
}
