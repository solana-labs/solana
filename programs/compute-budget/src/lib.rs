use solana_program_runtime::declare_process_instruction;

declare_process_instruction!(process_instruction, 150, |_invoke_context| {
    // Do nothing, compute budget instructions handled by the runtime
    Ok(())
});
