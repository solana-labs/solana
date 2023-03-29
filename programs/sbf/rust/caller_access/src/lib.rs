use {
    solana_program::{
        account_info::AccountInfo,
        entrypoint::ProgramResult,
        instruction::{AccountMeta, Instruction},
        msg,
        program::invoke,
        pubkey::Pubkey,
    },
    std::convert::TryInto,
};

solana_program::entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() == 32 {
        let key = Pubkey::new_from_array(instruction_data.try_into().unwrap());
        let ix = Instruction::new_with_bincode(key, &[2], vec![]);
        let mut lamports = accounts[0].lamports();
        let owner = &accounts[0].owner;
        let mut data = accounts[0].try_borrow_mut_data()?;
        let account =
            AccountInfo::new(&key, false, false, &mut lamports, &mut data, owner, true, 0);
        msg!("{:?} calling {:?}", program_id, key);
        invoke(&ix, &[account])?;
    } else {
        match instruction_data[0] {
            1 => {
                let ix = Instruction::new_with_bincode(
                    *program_id,
                    &accounts[1].key.to_bytes(),
                    vec![AccountMeta::new_readonly(*program_id, false)],
                );
                msg!("{:?} calling {:?}", program_id, program_id);
                invoke(&ix, accounts)?;
            }

            _ => msg!("Should never get here"),
        }
    }
    Ok(())
}
