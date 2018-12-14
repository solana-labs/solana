//! The `erc20` library implements a generic erc20-like token

extern crate bincode;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate solana_sdk;

use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;

mod token_program;

solana_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    info: &mut [KeyedAccount],
    input: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    solana_logger::setup();

    token_program::TokenProgram::process(program_id, info, input).map_err(|err| {
        error!("error: {:?}", err);
        ProgramError::GenericError
    })
}
