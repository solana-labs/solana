crate::declare_id!("Nonce11111111111111111111111111111111111111");

pub fn solana_nonce_program() -> (String, crate::pubkey::Pubkey) {
    ("solana_nonce_program".to_string(), id())
}
