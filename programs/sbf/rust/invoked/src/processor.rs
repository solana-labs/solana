//! Example Rust-based SBF program that issues a cross-program-invocation

#![cfg(feature = "program")]
#![allow(clippy::arithmetic_side_effects)]

use {
    crate::instructions::*,
    solana_program::{
        account_info::AccountInfo,
        bpf_loader,
        entrypoint::{ProgramResult, MAX_PERMITTED_DATA_INCREASE},
        log::sol_log_64,
        msg,
        program::{get_return_data, invoke, invoke_signed, set_return_data},
        program_error::ProgramError,
        pubkey::Pubkey,
        system_instruction,
    },
};

solana_program::entrypoint!(process_instruction);
#[allow(clippy::cognitive_complexity)]
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    msg!("Invoked program");

    if instruction_data.is_empty() {
        return Ok(());
    }

    assert_eq!(get_return_data(), None);

    match instruction_data[0] {
        VERIFY_TRANSLATIONS => {
            msg!("verify data translations");

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
            assert_eq!(accounts[ARGUMENT_INDEX].rent_epoch, u64::MAX);
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
            assert_eq!(accounts[INVOKED_ARGUMENT_INDEX].rent_epoch, u64::MAX);
            assert!(!accounts[INVOKED_ARGUMENT_INDEX].executable);

            assert_eq!(accounts[INVOKED_PROGRAM_INDEX].key, program_id);
            assert_eq!(accounts[INVOKED_PROGRAM_INDEX].owner, &bpf_loader::id());
            assert!(!accounts[INVOKED_PROGRAM_INDEX].is_signer);
            assert!(!accounts[INVOKED_PROGRAM_INDEX].is_writable);
            assert_eq!(accounts[INVOKED_PROGRAM_INDEX].rent_epoch, u64::MAX);
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
                sol_log_64(data[0] as u64, 0, 0, 0, 0);
            }
        }
        RETURN_OK => {
            msg!("Ok");
            return Ok(());
        }
        RETURN_ERROR => {
            msg!("return error");
            return Err(ProgramError::Custom(42));
        }
        DERIVED_SIGNERS => {
            msg!("verify derived signers");
            const INVOKED_PROGRAM_INDEX: usize = 0;
            const DERIVED_KEY1_INDEX: usize = 1;
            const DERIVED_KEY2_INDEX: usize = 2;
            const DERIVED_KEY3_INDEX: usize = 3;

            assert!(accounts[DERIVED_KEY1_INDEX].is_signer);
            assert!(!accounts[DERIVED_KEY2_INDEX].is_signer);
            assert!(!accounts[DERIVED_KEY3_INDEX].is_signer);

            let bump_seed2 = instruction_data[1];
            let bump_seed3 = instruction_data[2];
            let invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[
                    (accounts[DERIVED_KEY1_INDEX].key, true, false),
                    (accounts[DERIVED_KEY2_INDEX].key, true, true),
                    (accounts[DERIVED_KEY3_INDEX].key, false, true),
                ],
                vec![VERIFY_NESTED_SIGNERS],
            );
            invoke_signed(
                &invoked_instruction,
                accounts,
                &[
                    &[b"Lil'", b"Bits", &[bump_seed2]],
                    &[accounts[DERIVED_KEY2_INDEX].key.as_ref(), &[bump_seed3]],
                ],
            )?;
        }
        VERIFY_NESTED_SIGNERS => {
            msg!("verify nested derived signers");
            const DERIVED_KEY1_INDEX: usize = 0;
            const DERIVED_KEY2_INDEX: usize = 1;
            const DERIVED_KEY3_INDEX: usize = 2;

            assert!(!accounts[DERIVED_KEY1_INDEX].is_signer);
            assert!(accounts[DERIVED_KEY2_INDEX].is_signer);
            assert!(accounts[DERIVED_KEY3_INDEX].is_signer);
        }
        VERIFY_WRITER => {
            msg!("verify writable");
            const ARGUMENT_INDEX: usize = 0;

            assert!(!accounts[ARGUMENT_INDEX].is_writable);
        }
        VERIFY_PRIVILEGE_ESCALATION => {
            msg!("Verify privilege escalation");
        }
        VERIFY_PRIVILEGE_DEESCALATION => {
            msg!("verify privilege deescalation");
            const INVOKED_ARGUMENT_INDEX: usize = 0;
            assert!(!accounts[INVOKED_ARGUMENT_INDEX].is_signer);
            assert!(!accounts[INVOKED_ARGUMENT_INDEX].is_writable);
        }
        VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER => {
            msg!("verify privilege deescalation escalation signer");
            const INVOKED_PROGRAM_INDEX: usize = 0;
            const INVOKED_ARGUMENT_INDEX: usize = 1;

            assert!(!accounts[INVOKED_ARGUMENT_INDEX].is_signer);
            assert!(!accounts[INVOKED_ARGUMENT_INDEX].is_writable);
            let invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[(accounts[INVOKED_ARGUMENT_INDEX].key, true, false)],
                vec![VERIFY_PRIVILEGE_ESCALATION],
            );
            invoke(&invoked_instruction, accounts)?;
        }
        VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE => {
            msg!("verify privilege deescalation escalation writable");
            const INVOKED_PROGRAM_INDEX: usize = 0;
            const INVOKED_ARGUMENT_INDEX: usize = 1;

            assert!(!accounts[INVOKED_ARGUMENT_INDEX].is_signer);
            assert!(!accounts[INVOKED_ARGUMENT_INDEX].is_writable);
            let invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[(accounts[INVOKED_ARGUMENT_INDEX].key, false, true)],
                vec![VERIFY_PRIVILEGE_ESCALATION],
            );
            invoke(&invoked_instruction, accounts)?;
        }
        NESTED_INVOKE => {
            msg!("nested invoke");
            const ARGUMENT_INDEX: usize = 0;
            const INVOKED_ARGUMENT_INDEX: usize = 1;
            const INVOKED_PROGRAM_INDEX: usize = 2;

            assert!(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
            assert!(instruction_data.len() > 1);

            **accounts[INVOKED_ARGUMENT_INDEX].lamports.borrow_mut() -= 1;
            **accounts[ARGUMENT_INDEX].lamports.borrow_mut() += 1;
            let remaining_invokes = instruction_data[1];
            if remaining_invokes > 1 {
                msg!("Invoke again");
                let invoked_instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, true),
                        (accounts[INVOKED_ARGUMENT_INDEX].key, true, true),
                        (accounts[INVOKED_PROGRAM_INDEX].key, false, false),
                    ],
                    vec![NESTED_INVOKE, remaining_invokes - 1],
                );
                invoke(&invoked_instruction, accounts)?;
            } else {
                msg!("Last invoked");
                {
                    let mut data = accounts[INVOKED_ARGUMENT_INDEX].try_borrow_mut_data()?;
                    for i in 0..10 {
                        data[i as usize] = i;
                    }
                }
            }
        }
        WRITE_ACCOUNT => {
            msg!("write account");
            const ARGUMENT_INDEX: usize = 0;

            for i in 0..instruction_data[1] {
                accounts[ARGUMENT_INDEX].data.borrow_mut()[i as usize] = instruction_data[1];
            }
        }
        CREATE_AND_INIT => {
            msg!("Create and init data");
            {
                const FROM_INDEX: usize = 0;
                const DERIVED_KEY2_INDEX: usize = 1;

                let from_lamports = accounts[FROM_INDEX].lamports();
                let to_lamports = accounts[DERIVED_KEY2_INDEX].lamports();
                assert_eq!(accounts[DERIVED_KEY2_INDEX].data_len(), 0);
                assert!(solana_program::system_program::check_id(
                    accounts[DERIVED_KEY2_INDEX].owner
                ));

                let bump_seed2 = instruction_data[1];
                let instruction = system_instruction::create_account(
                    accounts[FROM_INDEX].key,
                    accounts[DERIVED_KEY2_INDEX].key,
                    1,
                    MAX_PERMITTED_DATA_INCREASE as u64,
                    program_id,
                );
                invoke_signed(
                    &instruction,
                    accounts,
                    &[&[b"Lil'", b"Bits", &[bump_seed2]]],
                )?;

                assert_eq!(accounts[FROM_INDEX].lamports(), from_lamports - 1);
                assert_eq!(accounts[DERIVED_KEY2_INDEX].lamports(), to_lamports + 1);
                assert_eq!(program_id, accounts[DERIVED_KEY2_INDEX].owner);
                assert_eq!(
                    accounts[DERIVED_KEY2_INDEX].data_len(),
                    MAX_PERMITTED_DATA_INCREASE
                );
                let mut data = accounts[DERIVED_KEY2_INDEX].try_borrow_mut_data()?;
                assert_eq!(data[0], 0);
                data[0] = 0x0e;
                assert_eq!(data[0], 0x0e);
                assert_eq!(data[MAX_PERMITTED_DATA_INCREASE - 1], 0);
                data[MAX_PERMITTED_DATA_INCREASE - 1] = 0x0f;
                assert_eq!(data[MAX_PERMITTED_DATA_INCREASE - 1], 0x0f);
                for i in 1..20 {
                    data[i] = i as u8;
                }
            }
        }
        SET_RETURN_DATA => {
            msg!("Set return data");

            set_return_data(b"Set by invoked");
        }
        ASSIGN_ACCOUNT_TO_CALLER => {
            msg!("Assigning account to caller");
            const ARGUMENT_INDEX: usize = 0;
            const CALLER_PROGRAM_ID: usize = 2;
            let account = &accounts[ARGUMENT_INDEX];
            let caller_program_id = accounts[CALLER_PROGRAM_ID].key;
            account.assign(caller_program_id);
        }
        _ => panic!(),
    }

    Ok(())
}
