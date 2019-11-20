pub mod date_instruction;
pub mod vest_instruction;
pub mod vest_processor;
pub mod vest_schedule;
pub mod vest_state;

const VEST_PROGRAM_ID: [u8; 32] = [
    7, 87, 23, 47, 219, 236, 238, 33, 137, 188, 215, 141, 32, 229, 155, 195, 133, 124, 23, 232,
    113, 153, 252, 252, 111, 5, 187, 128, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    VEST_PROGRAM_ID,
    "Vest111111111111111111111111111111111111111"
);
