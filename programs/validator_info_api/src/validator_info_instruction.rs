use crate::id;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction;
use vcard::VCard;

pub const MAX_VALIDATOR_INFO_LENGTH: u64 = 512;

/// Create a new, initialized validator info account
pub fn create_account(
    validator_pubkey: &Pubkey,
    info_account_pubkey: &Pubkey,
    validator_info: &VCard,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            validator_pubkey,
            info_account_pubkey,
            1,
            MAX_VALIDATOR_INFO_LENGTH,
            &id(),
        ),
        write_validator_info(validator_pubkey, info_account_pubkey, validator_info),
    ]
}

/// Write validator info to account. Must be signed by the validator keypair
pub fn write_validator_info(
    validator_pubkey: &Pubkey,
    info_account_pubkey: &Pubkey,
    validator_info: &VCard,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new_credit_only(*validator_pubkey, true),
        AccountMeta::new(*info_account_pubkey, false),
    ];
    let data = validator_info.to_string();
    Instruction::new(id(), &data, account_metas)
}
