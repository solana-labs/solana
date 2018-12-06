extern crate bincode;
extern crate chrono;
extern crate env_logger;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate solana_sdk;

mod budget_program;

use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use std::sync::{Once, ONCE_INIT};

use budget_program::process_instruction;

solana_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    argdata: &[u8],
    _input: &[u8],
    _tick_height: u64,
) -> Result<Vec<u8>, ProgramError> {
    static INIT: Once = ONCE_INIT;
    INIT.call_once(|| {
        // env_logger can only be initialized once
        env_logger::init();
    });

    trace!("process_instruction: {:?}", argdata);
    trace!("keyed_accounts: {:?}", keyed_accounts);
    process_instruction(keyed_accounts, argdata).map_err(|_| ProgramError::GenericError)?;
    Ok(vec![])
}
