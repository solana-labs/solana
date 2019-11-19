pub mod ownable_instruction;
pub mod ownable_processor;

const OWNABLE_PROGRAM_ID: [u8; 32] = [
    12, 6, 169, 236, 232, 53, 216, 159, 221, 186, 8, 8, 33, 45, 166, 249, 243, 55, 177, 184, 195,
    132, 141, 34, 63, 108, 219, 80, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    OWNABLE_PROGRAM_ID,
    "ownab1e111111111111111111111111111111111111"
);
