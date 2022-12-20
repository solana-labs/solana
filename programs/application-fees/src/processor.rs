use solana_program_runtime::invoke_context::InvokeContext;
use solana_sdk::{instruction::InstructionError, transaction_context::IndexOfAccount};

pub fn process_instruction(
    _first_instruction_account: IndexOfAccount,
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    Ok(())
}
