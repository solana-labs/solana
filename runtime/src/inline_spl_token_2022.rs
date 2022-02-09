/// Partial SPL Token declarations inlined to avoid an external dependency on the spl-token-2022 crate
use {
    crate::inline_spl_token::*,
    solana_sdk::pubkey::{Pubkey, PUBKEY_BYTES},
};

solana_sdk::declare_id!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

pub mod state {
    use super::*;

    const BASE_ACCOUNT_LENGTH: usize = crate::inline_spl_token::state::ACCOUNT_LENGTH;

    // `spl_token_program_2022::extension::AccountType::Account` ordinal value
    const ACCOUNTTYPE_ACCOUNT: u8 = 2;

    fn valid_account_data(account_data: &[u8]) -> bool {
        account_data.len() == BASE_ACCOUNT_LENGTH
            || ACCOUNTTYPE_ACCOUNT == *account_data.get(BASE_ACCOUNT_LENGTH).unwrap_or(&0)
    }

    pub fn unpack_account_owner(account_data: &[u8]) -> Option<&Pubkey> {
        if valid_account_data(account_data) {
            Some(bytemuck::from_bytes(
                &account_data
                    [SPL_TOKEN_ACCOUNT_OWNER_OFFSET..SPL_TOKEN_ACCOUNT_OWNER_OFFSET + PUBKEY_BYTES],
            ))
        } else {
            None
        }
    }

    pub fn unpack_account_mint(account_data: &[u8]) -> Option<&Pubkey> {
        if valid_account_data(account_data) {
            Some(bytemuck::from_bytes(
                &account_data
                    [SPL_TOKEN_ACCOUNT_MINT_OFFSET..SPL_TOKEN_ACCOUNT_MINT_OFFSET + PUBKEY_BYTES],
            ))
        } else {
            None
        }
    }
}
