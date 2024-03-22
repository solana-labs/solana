use solana_program::{
    account_info::{AccountInfo, next_account_info}, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
    program::invoke, system_instruction,
};

entrypoint!(process_instruction);


fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8]
) -> ProgramResult {
    let amount = u64::from_be_bytes(data[0..8].try_into().unwrap());
    let accounts_iter = &mut accounts.iter();
    let payer = next_account_info(accounts_iter)?;
    let recipient = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    invoke(
        &system_instruction::transfer(payer.key, recipient.key, amount),
        &[payer.clone(), recipient.clone(), system_program.clone()],
    )?;

    Ok(())
}