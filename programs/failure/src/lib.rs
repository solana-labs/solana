use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

pub const FAILURE_PROGRAM_ID: [u8; 32] = [
    3, 188, 64, 34, 171, 255, 206, 240, 89, 4, 11, 161, 30, 250, 18, 135, 195, 82, 6, 72, 220, 142,
    53, 26, 45, 144, 70, 112, 0, 0, 0, 0,
];

solana_sdk::declare_program!(
    FAILURE_PROGRAM_ID,
    "FaiLure111111111111111111111111111111111111",
    solana_failure_program,
    process_instruction
);

fn process_instruction(
    _program_id: &Pubkey,
    _keyed_accounts: &mut [KeyedAccount],
    _data: &[u8],
) -> Result<(), InstructionError> {
    Err(InstructionError::GenericError)
}
