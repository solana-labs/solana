use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    instruction::{AccountMeta, Instruction},
    program::invoke,
    pubkey::Pubkey,
    system_program,
};

#[cfg(not(feature = "exclude_entrypoint"))]
entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> entrypoint::ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let from_account = next_account_info(accounts_iter)?;
    let to_account = next_account_info(accounts_iter)?;

    let account_metas = vec![
        AccountMeta::new(*from_account.key, true),
        AccountMeta::new(*to_account.key, false),
    ];

    let instruction =
        Instruction::new_with_bytes(system_program::id(), instruction_data, account_metas);

    invoke(&instruction, accounts)
}
