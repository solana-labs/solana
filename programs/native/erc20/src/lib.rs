//! The `erc20` library implements a generic erc20-like token

extern crate bincode;
extern crate env_logger;
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
use std::sync::{Once, ONCE_INIT};

mod token_program;

solana_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    info: &mut [KeyedAccount],
    argdata: &[u8],
    _input: &[u8],
    _tick_height: u64,
) -> Result<Vec<u8>, ProgramError> {
    // env_logger can only be initialized once
    static INIT: Once = ONCE_INIT;
    INIT.call_once(env_logger::init);

    token_program::TokenProgram::process(program_id, info, argdata).map_err(|err| {
        error!("error: {:?}", err);
        ProgramError::GenericError
    })?;
    Ok(vec![])
}
