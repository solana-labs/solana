/// Partial SPL Token declarations inlined to avoid an external dependency on the spl-token crate
use solana_sdk::pubkey::{Pubkey, PUBKEY_BYTES};

solana_sdk::declare_id!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

pub(crate) mod new_token_program {
    solana_sdk::declare_id!("nTok2oJvx1CgbYA2SznfJLmnKLEL6sYdh2ypZms2nhm");
}

/*
    spl_token::state::Account {
        mint: Pubkey,
        owner: Pubkey,
        amount: u64,
        delegate: COption<Pubkey>,
        state: AccountState,
        is_native: COption<u64>,
        delegated_amount: u64,
        close_authority: COption<Pubkey>,
    }
*/
pub const SPL_TOKEN_ACCOUNT_MINT_OFFSET: usize = 0;
pub const SPL_TOKEN_ACCOUNT_OWNER_OFFSET: usize = 32;

pub mod state {
    use super::*;

    pub(crate) const ACCOUNT_LENGTH: usize = 165;

    #[cfg(test)]
    pub struct Account;
    #[cfg(test)]
    impl Account {
        pub fn get_packed_len() -> usize {
            ACCOUNT_LENGTH
        }
    }

    pub fn unpack_account_owner(account_data: &[u8]) -> Option<&Pubkey> {
        if account_data.len() != ACCOUNT_LENGTH {
            None
        } else {
            Some(bytemuck::from_bytes(
                &account_data
                    [SPL_TOKEN_ACCOUNT_OWNER_OFFSET..SPL_TOKEN_ACCOUNT_OWNER_OFFSET + PUBKEY_BYTES],
            ))
        }
    }

    pub fn unpack_account_mint(account_data: &[u8]) -> Option<&Pubkey> {
        if account_data.len() != ACCOUNT_LENGTH {
            None
        } else {
            Some(bytemuck::from_bytes(
                &account_data
                    [SPL_TOKEN_ACCOUNT_MINT_OFFSET..SPL_TOKEN_ACCOUNT_MINT_OFFSET + PUBKEY_BYTES],
            ))
        }
    }
}

pub mod native_mint {
    solana_sdk::declare_id!("So11111111111111111111111111111111111111112");

    /*
        Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        }
    */
    pub const ACCOUNT_DATA: [u8; 82] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
}
