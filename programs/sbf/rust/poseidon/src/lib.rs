//! Example SBF program using Poseidon syscall

use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    msg,
    poseidon::{prelude::poseidon_hash, PoseidonSyscallError},
    program_error::ProgramError,
    pubkey::Pubkey,
};

fn test_poseidon_hash() -> Result<(), PoseidonSyscallError> {
    let input1 = [1u8; 32];
    let input2 = [2u8; 32];

    let hash = poseidon_hash(&[&input1, &input2])?;

    assert_eq!(
        hash.to_bytes(),
        [
            40, 7, 251, 60, 51, 30, 115, 141, 251, 200, 13, 46, 134, 91, 113, 170, 131, 90, 53,
            175, 9, 61, 242, 164, 127, 33, 249, 65, 253, 131, 35, 116
        ]
    );

    Ok(())
}

solana_program::entrypoint!(process_instruction);
pub fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    msg!("poseidon_hash");

    test_poseidon_hash().map_err(|e| ProgramError::from(u64::from(e)))?;

    Ok(())
}

#[cfg(test)]
mod test {
    #[test]
    fn test_poseidon_hash() {
        super::test_poseidon_hash().unwrap();
    }
}
