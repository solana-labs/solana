use log::*;
use solana_sdk::{
    instruction::InstructionError, process_instruction::InvokeContext, pubkey::Pubkey,
};

solana_sdk::declare_program!(
    "Noop111111111111111111111111111111111111111",
    solana_noop_program,
    process_instruction
);

pub fn process_instruction(
    program_id: &Pubkey,
    data: &[u8],
    _invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    solana_logger::setup();
    trace!("noop: program_id: {:?}", program_id);
    trace!("noop: data: {:?}", data);
    Ok(())
}
