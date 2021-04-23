use solana_sdk::{
    instruction::InstructionError, keyed_account::KeyedAccount, process_instruction::InvokeContext,
    pubkey::Pubkey,
};

solana_sdk::declare_program!(
    "FaiLure111111111111111111111111111111111111",
    solana_failure_program,
    process_instruction
);

fn process_instruction(
    _program_id: &Pubkey,
    _keyed_accounts: &[KeyedAccount],
    _data: &[u8],
    _invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    Err(InstructionError::Custom(0))
}
