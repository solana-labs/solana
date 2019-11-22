pub const BS58_STRING: &str = "MoveLdr111111111111111111111111111111111111";
crate::declare_id!(BS58_STRING);

pub fn solana_move_loader_program() -> (String, crate::pubkey::Pubkey) {
    ("solana_move_loader_program".to_string(), id())
}
