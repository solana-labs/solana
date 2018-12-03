#[macro_use]
extern crate solana_sdk;

use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;

solana_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    tick_height: u64,
) -> Result<(), ProgramError> {
    println!("noop: program_id: {:?}", program_id);
    println!("noop: keyed_accounts: {:#?}", keyed_accounts);
    println!("noop: data: {:?}", data);
    println!("noop: tick_height: {:?}", tick_height);
    Ok(())
}
