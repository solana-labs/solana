use {solana_program_runtime::invoke_context::InvokeContext, solana_sdk::feature_set};

pub fn process_instruction(
    invoke_context: &mut InvokeContext,
) -> Result<(), Box<dyn std::error::Error>> {
    // Consume compute units if feature `native_programs_consume_cu` is activated,
    if invoke_context
        .feature_set
        .is_active(&feature_set::native_programs_consume_cu::id())
    {
        invoke_context.consume_checked(150)?;
    }

    // Do nothing, compute budget instructions handled by the runtime
    Ok(())
}
