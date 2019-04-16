pub mod budget_expr;
pub mod budget_instruction;
pub mod budget_processor;
pub mod budget_state;

use solana_sdk::pubkey::Pubkey;

const BUDGET_PROGRAM_ID: [u8; 32] = [
    129, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&BUDGET_PROGRAM_ID)
}

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == BUDGET_PROGRAM_ID
}
