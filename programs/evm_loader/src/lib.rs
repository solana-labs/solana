mod processor;

solana_sdk::declare_program!(
    "EVM1111111111111111111111111111111111111111",
    solana_evm_program,
    processor::process_instruction
);
