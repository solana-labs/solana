//! Example Rust-based SBF program that supports the deprecated loader

#![allow(unreachable_code)]
#![allow(clippy::arithmetic_side_effects)]

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo,
    bpf_loader,
    entrypoint_deprecated::ProgramResult,
    instruction::{AccountMeta, Instruction},
    log::*,
    msg,
    program::invoke,
    pubkey::Pubkey,
};

pub const REALLOC: u8 = 1;
pub const REALLOC_EXTEND_FROM_SLICE: u8 = 12;
pub const TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS: u8 = 28;
pub const TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_NESTED: u8 = 29;

#[derive(Debug, PartialEq)]
struct SStruct {
    x: u64,
    y: u64,
    z: u64,
}

#[inline(never)]
fn return_sstruct() -> SStruct {
    SStruct { x: 1, y: 2, z: 3 }
}

#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    // Full panic reporting
    msg!(&format!("{info}"));
}

solana_program::entrypoint_deprecated!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    msg!("Program identifier:");
    program_id.log();

    assert!(!bpf_loader::check_id(program_id));

    // test_sol_alloc_free_no_longer_deployable calls this program with
    // bpf_loader instead of bpf_loader_deprecated, so instruction_data isn't
    // deserialized correctly and is empty.
    match instruction_data.first() {
        Some(&REALLOC) => {
            let (bytes, _) = instruction_data[2..].split_at(std::mem::size_of::<usize>());
            let new_len = usize::from_le_bytes(bytes.try_into().unwrap());
            msg!("realloc to {}", new_len);
            let account = &accounts[0];
            account.realloc(new_len, false)?;
            assert_eq!(new_len, account.data_len());
        }
        Some(&REALLOC_EXTEND_FROM_SLICE) => {
            msg!("realloc extend from slice deprecated");
            let data = &instruction_data[1..];
            let account = &accounts[0];
            let prev_len = account.data_len();
            account.realloc(prev_len + data.len(), false)?;
            account.data.borrow_mut()[prev_len..].copy_from_slice(data);
        }
        Some(&TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS) => {
            msg!("DEPRECATED TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS");
            const ARGUMENT_INDEX: usize = 1;
            const CALLEE_PROGRAM_INDEX: usize = 3;
            let account = &accounts[ARGUMENT_INDEX];
            let callee_program_id = accounts[CALLEE_PROGRAM_INDEX].key;

            let expected = {
                let data = &instruction_data[1..];
                let prev_len = account.data_len();
                // when direct mapping is off, this will accidentally clobber
                // whatever comes after the data slice (owner, executable, rent
                // epoch etc). When direct mapping is on, you get an
                // InvalidRealloc error.
                account.realloc(prev_len + data.len(), false)?;
                account.data.borrow_mut()[prev_len..].copy_from_slice(data);
                account.data.borrow().to_vec()
            };

            let mut instruction_data = vec![TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_NESTED];
            instruction_data.extend_from_slice(&expected);
            invoke(
                &create_instruction(
                    *callee_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (callee_program_id, false, false),
                    ],
                    instruction_data,
                ),
                accounts,
            )
            .unwrap();
        }
        Some(&TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_NESTED) => {
            msg!("DEPRECATED LOADER TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_NESTED");
            const ARGUMENT_INDEX: usize = 0;
            let account = &accounts[ARGUMENT_INDEX];
            assert_eq!(*account.data.borrow(), &instruction_data[1..]);
        }
        _ => {
            {
                // Log the provided account keys and instruction input data.  In the case of
                // the no-op program, no account keys or input data are expected but real
                // programs will have specific requirements so they can do their work.
                msg!("Account keys and instruction input data:");
                sol_log_params(accounts, instruction_data);

                // Test - use std methods, unwrap

                // valid bytes, in a stack-allocated array
                let sparkle_heart = [240, 159, 146, 150];
                let result_str = std::str::from_utf8(&sparkle_heart).unwrap();
                assert_eq!(4, result_str.len());
                assert_eq!("💖", result_str);
                msg!(result_str);
            }

            {
                // Test - struct return

                let s = return_sstruct();
                assert_eq!(s.x + s.y + s.z, 6);
            }

            {
                // Test - arch config
                #[cfg(not(target_os = "solana"))]
                panic!();
            }
        }
    }

    Ok(())
}

pub fn create_instruction(
    program_id: Pubkey,
    arguments: &[(&Pubkey, bool, bool)],
    data: Vec<u8>,
) -> Instruction {
    let accounts = arguments
        .iter()
        .map(|(key, is_writable, is_signer)| {
            if *is_writable {
                AccountMeta::new(**key, *is_signer)
            } else {
                AccountMeta::new_readonly(**key, *is_signer)
            }
        })
        .collect();
    Instruction {
        program_id,
        accounts,
        data,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_return_sstruct() {
        assert_eq!(SStruct { x: 1, y: 2, z: 3 }, return_sstruct());
    }
}
