// Partial SPL Token v2.0.x declarations inlined to avoid an external dependency on the spl-token crate
solana_sdk::declare_id!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

pub mod state {
    const LEN: usize = 165;
    pub struct Account;
    impl Account {
        pub fn get_packed_len() -> usize {
            LEN
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
