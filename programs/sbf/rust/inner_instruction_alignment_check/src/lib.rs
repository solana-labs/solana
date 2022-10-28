//! Example Rust-based SBF noop program

use solana_program::{
    account_info::AccountInfo,
    custom_heap_default,
    entrypoint_deprecated::ProgramResult,
    instruction::{AccountMeta, Instruction},
    msg,
    program::invoke,
    pubkey::Pubkey,
};

solana_program::entrypoint_deprecated!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let to_call = accounts[0].key;
    let infos = accounts;
    let instruction = Instruction {
        accounts: vec![AccountMeta {
            pubkey: *accounts[1].key,
            is_signer: accounts[1].is_signer,
            is_writable: accounts[1].is_writable,
        }],
        data: instruction_data.to_owned(),
        program_id: *to_call,
    };

    let _ = invoke(&instruction, infos);
    let _ = invoke(&instruction, infos);

    Ok(())
}

custom_heap_default!();

#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    // Full panic reporting
    msg!(&format!("{}", info));
}
