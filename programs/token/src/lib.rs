use bincode::serialize;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;
use solana_sdk::transaction::InstructionError;

mod token_program;

solana_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    info: &mut [KeyedAccount],
    input: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    token_program::TokenProgram::process(program_id, info, input).map_err(|e| {
        error!("error: {:?}", e);
        InstructionError::CustomError(serialize(&e).unwrap())
    })
}
