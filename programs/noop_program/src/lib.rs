use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;

// TODO
pub const NOOP_PROGRAM_ID: [u8; 32] = [
    3, 147, 111, 103, 210, 47, 14, 213, 108, 116, 49, 115, 232, 171, 14, 111, 167, 140, 221, 234,
    33, 70, 185, 192, 42, 31, 141, 152, 0, 0, 0, 0,
];

// TODO
solana_sdk::declare_program!(
    NOOP_PROGRAM_ID,
    "Exchange11111111111111111111111111111111111",
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
