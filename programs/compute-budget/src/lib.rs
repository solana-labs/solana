use {
    solana_program_runtime::invoke_context::InvokeContext,
    solana_sdk::{instruction::InstructionError, transaction_context::IndexOfAccount},
};

pub fn process_instruction(
    _first_instruction_account: IndexOfAccount,
    _invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    // Do nothing, compute budget instructions handled by the runtime
    Ok(())
}
