use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

solana_sdk::declare_program!(
    "FaiLure111111111111111111111111111111111111",
    solana_failure_program,
    process_instruction
);

fn process_instruction(
    _program_id: &Pubkey,
    _keyed_accounts: &[KeyedAccount],
    _data: &[u8],
) -> Result<(), InstructionError> {
    Err(InstructionError::GenericError)
}
