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

use solana_sdk::account::{Account, KeyedAccount};
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use std::sync::{Once, ONCE_INIT};

mod token_program;

const ERC20_NAME: &str = "solana_erc20";
const ERC20_PROGRAM_ID: [u8; 32] = [
    131, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&ERC20_PROGRAM_ID)
}

pub fn account() -> Account {
    Account {
        tokens: 1,
        owner: id(),
        userdata: ERC20_NAME.as_bytes().to_vec(),
        executable: true,
        loader: native_loader::id(),
    }
}

solana_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    info: &mut [KeyedAccount],
    input: &[u8],
    _tick_height: u64,
) -> bool {
    // env_logger can only be initialized once
    static INIT: Once = ONCE_INIT;
    INIT.call_once(env_logger::init);

    match token_program::TokenProgram::process(program_id, info, input) {
        Err(err) => {
            error!("error: {:?}", err);
            false
        }
        Ok(_) => true,
    }
}
