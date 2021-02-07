use log::*;
use safecoin_sdk::{
    instruction::InstructionError, keyed_account::KeyedAccount, process_instruction::InvokeContext,
    pubkey::Pubkey,
};

safecoin_sdk::declare_program!(
    "Noop111111111111111111111111111111111111111",
    safecoin_noop_program,
    process_instruction
);

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    data: &[u8],
    _invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    safecoin_logger::setup();
    trace!("noop: program_id: {:?}", program_id);
    trace!("noop: keyed_accounts: {:#?}", keyed_accounts);
    trace!("noop: data: {:?}", data);
    Ok(())
}
