mod budget_processor;

use crate::budget_processor::process_instruction;
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

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);
    process_instruction(program_id, keyed_accounts, data, tick_height)
}
