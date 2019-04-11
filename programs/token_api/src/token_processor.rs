use crate::token_state::TokenState;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

pub fn process_instruction(
    program_id: &Pubkey,
    info: &mut [KeyedAccount],
    input: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    TokenState::process(program_id, info, input).map_err(|e| {
        error!("error: {:?}", e);
        InstructionError::CustomError(e as u32)
    })
}
