crate::declare_id!("11111111111111111111111111111111");

pub fn solana_system_program() -> (String, crate::pubkey::Pubkey) {
    ("solana_system_program".to_string(), id())
}
