pub mod vote_instruction;
pub mod vote_state;

use crate::vote_instruction::process_instruction;

const VOTE_PROGRAM_ID: [u8; 32] = [
    7, 97, 72, 29, 53, 116, 116, 187, 124, 77, 118, 36, 235, 211, 189, 179, 216, 53, 94, 115, 209,
    16, 67, 252, 13, 163, 83, 128, 0, 0, 0, 0,
];

solana_sdk::declare_program!(
    VOTE_PROGRAM_ID,
    "Vote111111111111111111111111111111111111111",
    solana_vote_program,
    process_instruction
);
