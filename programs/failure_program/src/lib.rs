use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

// TODO
pub const FAILURE_PROGRAM_ID: [u8; 32] = [
    3, 147, 111, 103, 210, 47, 14, 213, 108, 116, 49, 115, 232, 171, 14, 111, 167, 140, 221, 234,
    33, 70, 185, 192, 42, 31, 141, 152, 0, 0, 0, 0,
];

// TODO
solana_sdk::declare_program!(
    FAILURE_PROGRAM_ID,
    "Exchange11111111111111111111111111111111111",
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
