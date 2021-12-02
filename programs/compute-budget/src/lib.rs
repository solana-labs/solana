use solana_program_runtime::invoke_context::InvokeContext;
use solana_sdk::instruction::InstructionError;

pub fn process_instruction(
    _first_instruction_account: usize,
    _data: &[u8],
    _invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    // Do nothing, compute budget instructions handled by the runtime
    Ok(())
}
