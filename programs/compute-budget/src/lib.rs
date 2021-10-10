use solana_sdk::{instruction::InstructionError, process_instruction::InvokeContext};

pub fn process_instruction(
    _first_instruction_account: usize,
    _data: &[u8],
    _invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    // Do nothing, compute budget instructions handled by the runtime
    Ok(())
}
