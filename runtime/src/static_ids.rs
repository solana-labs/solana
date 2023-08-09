use {
    crate::inline_spl_associated_token_account,
    solana_accounts_db::{inline_spl_token, inline_spl_token_2022},
    solana_sdk::pubkey::Pubkey,
};

lazy_static! {
    /// Vector of static token & mint IDs
    pub static ref STATIC_IDS: Vec<Pubkey> = vec![
        inline_spl_associated_token_account::id(),
        inline_spl_associated_token_account::program_v1_1_0::id(),
        inline_spl_token::id(),
        inline_spl_token::native_mint::id(),
        inline_spl_token_2022::id(),
    ];
}
