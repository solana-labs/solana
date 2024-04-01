use {
    solana_inline_spl::{associated_token_account, token, token_2022},
    solana_sdk::pubkey::Pubkey,
};

lazy_static! {
    /// Vector of static token & mint IDs
    pub static ref STATIC_IDS: Vec<Pubkey> = vec![
        associated_token_account::id(),
        associated_token_account::program_v1_1_0::id(),
        token::id(),
        token::native_mint::id(),
        token_2022::id(),
    ];
}
