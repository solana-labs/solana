const MOVE_LOADER_PROGRAM_ID: [u8; 32] = [
    5, 84, 172, 160, 172, 5, 64, 41, 134, 4, 81, 31, 45, 11, 30, 64, 219, 238, 140, 38, 194, 100,
    192, 219, 156, 94, 62, 208, 0, 0, 0, 0,
];

crate::solana_name_id!(
    MOVE_LOADER_PROGRAM_ID,
    "MoveLdr111111111111111111111111111111111111"
);

pub fn solana_move_loader_program() -> (String, crate::pubkey::Pubkey) {
    ("solana_move_loader_program".to_string(), id())
}
