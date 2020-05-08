//! @brief Example Rust-based BPF program that issues a cross-program-invocation

#![allow(unreachable_code)]

pub mod instruction;

extern crate solana_sdk;

use crate::instruction::*;
use solana_sdk::{
    account_info::AccountInfo,
    bpf_loader, entrypoint,
    entrypoint::ProgramResult,
    info,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
};

entrypoint!(process_instruction);
#[allow(clippy::cognitive_complexity)]
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    info!("Invoked program");

    match instruction_data[0] {
        TEST_VERIFY_TRANSLATIONS => {
            info!("verify data translations");

            const ARGUMENT_INDEX: usize = 0;
            const INVOKED_ARGUMENT_INDEX: usize = 1;
            const INVOKED_PROGRAM_INDEX: usize = 2;
            const INVOKED_PROGRAM_DUP_INDEX: usize = 3;

            assert_eq!(&instruction_data[1..], &[1, 2, 3, 4, 5]);
            assert_eq!(accounts.len(), 4);

            assert_eq!(accounts[ARGUMENT_INDEX].lamports(), 42);
            assert_eq!(accounts[ARGUMENT_INDEX].data_len(), 100);
            assert!(accounts[ARGUMENT_INDEX].is_signer);
            assert!(accounts[ARGUMENT_INDEX].is_writable);
            assert_eq!(accounts[ARGUMENT_INDEX].rent_epoch, 1);
            assert!(!accounts[ARGUMENT_INDEX].executable);
            {
                let data = accounts[ARGUMENT_INDEX].try_borrow_data()?;
                for i in 0..100 {
                    assert_eq!(data[i as usize], i);
                }
            }

            assert_eq!(
                accounts[INVOKED_ARGUMENT_INDEX].owner,
                accounts[INVOKED_PROGRAM_INDEX].key
            );
            assert_eq!(accounts[INVOKED_ARGUMENT_INDEX].lamports(), 10);
            assert_eq!(accounts[INVOKED_ARGUMENT_INDEX].data_len(), 10);
            assert!(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
            assert!(accounts[INVOKED_ARGUMENT_INDEX].is_writable);
            assert_eq!(accounts[INVOKED_ARGUMENT_INDEX].rent_epoch, 1);
            assert!(!accounts[INVOKED_ARGUMENT_INDEX].executable);

            assert_eq!(accounts[INVOKED_PROGRAM_INDEX].key, program_id);
            assert_eq!(accounts[INVOKED_PROGRAM_INDEX].owner, &bpf_loader::id());
            assert!(!accounts[INVOKED_PROGRAM_INDEX].is_signer);
            assert!(!accounts[INVOKED_PROGRAM_INDEX].is_writable);
            assert_eq!(accounts[INVOKED_PROGRAM_INDEX].rent_epoch, 1);
            assert!(accounts[INVOKED_PROGRAM_INDEX].executable);

            assert_eq!(
                accounts[INVOKED_PROGRAM_INDEX].key,
                accounts[INVOKED_PROGRAM_DUP_INDEX].key
            );
            assert_eq!(
                accounts[INVOKED_PROGRAM_INDEX].owner,
                accounts[INVOKED_PROGRAM_DUP_INDEX].owner
            );
            assert_eq!(
                accounts[INVOKED_PROGRAM_INDEX].lamports,
                accounts[INVOKED_PROGRAM_DUP_INDEX].lamports
            );
            assert_eq!(
                accounts[INVOKED_PROGRAM_INDEX].is_signer,
                accounts[INVOKED_PROGRAM_DUP_INDEX].is_signer
            );
            assert_eq!(
                accounts[INVOKED_PROGRAM_INDEX].is_writable,
                accounts[INVOKED_PROGRAM_DUP_INDEX].is_writable
            );
            assert_eq!(
                accounts[INVOKED_PROGRAM_INDEX].rent_epoch,
                accounts[INVOKED_PROGRAM_DUP_INDEX].rent_epoch
            );
            assert_eq!(
                accounts[INVOKED_PROGRAM_INDEX].executable,
                accounts[INVOKED_PROGRAM_DUP_INDEX].executable
            );
            {
                let data = accounts[INVOKED_PROGRAM_INDEX].try_borrow_data()?;
                assert!(accounts[INVOKED_PROGRAM_DUP_INDEX]
                    .try_borrow_mut_data()
                    .is_err());
                info!(data[0], 0, 0, 0, 0);
            }
        }
        TEST_RETURN_ERROR => {
            info!("return error");
            return Err(ProgramError::Custom(42));
        }
        TEST_DERIVED_SIGNERS => {
            info!("verify derived signers");
            const INVOKED_PROGRAM_INDEX: usize = 0;
            const DERIVED_KEY1_INDEX: usize = 1;
            const DERIVED_KEY2_INDEX: usize = 2;
            const DERIVED_KEY3_INDEX: usize = 3;

            assert!(accounts[DERIVED_KEY1_INDEX].is_signer);
            assert!(!accounts[DERIVED_KEY2_INDEX].is_signer);
            assert!(!accounts[DERIVED_KEY3_INDEX].is_signer);

            let invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[
                    (accounts[DERIVED_KEY1_INDEX].key, true, false),
                    (accounts[DERIVED_KEY2_INDEX].key, true, true),
                    (accounts[DERIVED_KEY3_INDEX].key, false, true),
                ],
                vec![TEST_VERIFY_NESTED_SIGNERS],
            );
            invoke_signed(
                &invoked_instruction,
                accounts,
                &[&["Lil'", "Bits"], &["Gar Ma Nar Nar"]],
            )?;
        }
        TEST_VERIFY_NESTED_SIGNERS => {
            info!("verify nested derived signers");
            const DERIVED_KEY1_INDEX: usize = 0;
            const DERIVED_KEY2_INDEX: usize = 1;
            const DERIVED_KEY3_INDEX: usize = 2;

            assert!(!accounts[DERIVED_KEY1_INDEX].is_signer);
            assert!(accounts[DERIVED_KEY2_INDEX].is_signer);
            assert!(accounts[DERIVED_KEY3_INDEX].is_signer);
        }
        TEST_VERIFY_WRITER => {
            info!("verify writable");
            const ARGUMENT_INDEX: usize = 0;

            assert!(!accounts[ARGUMENT_INDEX].is_writable);
        }
        TEST_NESTED_INVOKE => {
            info!("nested invoke");

            const ARGUMENT_INDEX: usize = 0;
            const INVOKED_ARGUMENT_INDEX: usize = 1;
            const INVOKED_PROGRAM_INDEX: usize = 3;

            assert!(accounts[INVOKED_ARGUMENT_INDEX].is_signer);

            **accounts[INVOKED_ARGUMENT_INDEX].lamports.borrow_mut() -= 1;
            **accounts[ARGUMENT_INDEX].lamports.borrow_mut() += 1;
            if accounts.len() > 2 {
                info!("Invoke again");
                let invoked_instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, true),
                        (accounts[INVOKED_ARGUMENT_INDEX].key, true, true),
                    ],
                    vec![TEST_NESTED_INVOKE],
                );
                invoke(&invoked_instruction, accounts)?;
            } else {
                info!("Last invoked");
                {
                    let mut data = accounts[INVOKED_ARGUMENT_INDEX].try_borrow_mut_data()?;
                    for i in 0..10 {
                        data[i as usize] = i;
                    }
                }
            }
        }
        _ => panic!(),
    }

    Ok(())
}

solana_sdk_bpf_test::stubs!();
