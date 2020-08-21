use solana_sdk::pubkey::Pubkey;
use solana_sdk::{account::KeyedAccount, instruction::InstructionError};

pub fn process_instruction(
    _program_id: &Pubkey,
    _keyed_accounts: &[KeyedAccount],
    _data: &[u8],
) -> Result<(), InstructionError> {
    // Should be already checked by now.
    Ok(())
}

solana_sdk::declare_program!(
    solana_sdk::secp256k1_program::ID,
    solana_keccak_secp256k1_program,
    process_instruction
);

#[cfg(test)]
pub mod test {}
