use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;
use solana_sdk::transaction::InstructionError;

solana_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();
    info!("noop: program_id: {:?}", program_id);
    info!("noop: keyed_accounts: {:#?}", keyed_accounts);
    info!("noop: data: {:?}", data);
    info!("noop: tick_height: {:?}", tick_height);
    Ok(())
}
