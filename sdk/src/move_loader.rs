crate::declare_id!("MoveLdr111111111111111111111111111111111111");

pub fn solana_move_loader_program() -> (String, crate::pubkey::Pubkey) {
    ("solana_move_loader_program".to_string(), id())
}
