const ID: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

crate::solana_name_id!(ID, "11111111111111111111111111111111");

pub fn solana_system_program() -> (String, crate::pubkey::Pubkey) {
    ("solana_system_program".to_string(), id())
}
