use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
    sysvar::{clock::Clock, Sysvar}, program::set_return_data
};

entrypoint!(process_instruction);

fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {

    let time_now = Clock::get().unwrap().unix_timestamp;
    let return_data = time_now.to_be_bytes();
    set_return_data(&return_data);
    Ok(())
}
