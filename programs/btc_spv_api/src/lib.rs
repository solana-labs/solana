pub mod spv_instruction;
pub mod spv_state;
pub mod spv_processor;
pub mod utils;
pub mod header_store;

pub const BTC_SPV_PROGRAM_ID: [u8; 32] = [
    2, 202, 42, 59, 228, 51, 182, 147, 162, 245, 234, 78, 205, 37, 131, 154, 110, 252, 154, 254,
    190, 13, 90, 231, 198, 144, 239, 96, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    BTC_SPV_PROGRAM_ID,
    "BtcSpv1111111111111111111111111111111111111"
);
