use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

pub const NOOP_PROGRAM_ID: [u8; 32] = [
    5, 150, 31, 54, 19, 205, 142, 201, 161, 38, 97, 31, 144, 212, 37, 82, 93, 58, 178, 5, 131, 178,
    31, 101, 138, 251, 91, 128, 0, 0, 0, 0,
];

solana_sdk::declare_program!(
    NOOP_PROGRAM_ID,
    "Noop111111111111111111111111111111111111111",
    solana_noop_program,
    process_instruction
);

fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();
    trace!("noop: program_id: {:?}", program_id);
    trace!("noop: keyed_accounts: {:#?}", keyed_accounts);
    trace!("noop: data: {:?}", data);
    Ok(())
}
