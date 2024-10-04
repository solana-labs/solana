use {
    borsh::{BorshDeserialize, BorshSerialize},
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        entrypoint, msg,
        pubkey::Pubkey,
    },
};

/// The type of state managed by this program. The type defined here
/// must match the `GreetingAccount` type defined by the client.
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct GreetingAccount {
    /// The number of greetings that have been sent to this account.
    pub counter: u32,
}

entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> entrypoint::ProgramResult {
    // Get the account that stores greeting count information.
    let accounts_iter = &mut accounts.iter();
    let account = next_account_info(accounts_iter)?;

    msg!("account.owner");
    account.owner.log();
    msg!("program_id");
    program_id.log();
    Ok(())
}
