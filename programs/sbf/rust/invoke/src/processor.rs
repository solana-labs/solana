//! Example Rust-based SBF program that issues a cross-program-invocation

#![cfg(feature = "program")]
#![allow(unreachable_code)]
#![allow(clippy::arithmetic_side_effects)]

use {
    crate::instructions::*,
    solana_program::{
        account_info::AccountInfo,
        bpf_loader_deprecated,
        entrypoint::{ProgramResult, MAX_PERMITTED_DATA_INCREASE},
        instruction::Instruction,
        msg,
        program::{get_return_data, invoke, invoke_signed, set_return_data},
        program_error::ProgramError,
        pubkey::{Pubkey, PubkeyError},
        syscalls::{
            MAX_CPI_ACCOUNT_INFOS, MAX_CPI_INSTRUCTION_ACCOUNTS, MAX_CPI_INSTRUCTION_DATA_LEN,
        },
        system_instruction, system_program,
    },
    solana_sbf_rust_invoked::instructions::*,
    solana_sbf_rust_realloc::instructions::*,
    std::{cell::RefCell, mem, rc::Rc, slice},
};

fn do_nested_invokes(num_nested_invokes: u64, accounts: &[AccountInfo]) -> ProgramResult {
    assert!(accounts[ARGUMENT_INDEX].is_signer);

    let pre_argument_lamports = accounts[ARGUMENT_INDEX].lamports();
    let pre_invoke_argument_lamports = accounts[INVOKED_ARGUMENT_INDEX].lamports();
    {
        let mut lamports = (*accounts[ARGUMENT_INDEX].lamports).borrow_mut();
        **lamports = (*lamports).saturating_sub(5);
        let mut lamports = (*accounts[INVOKED_ARGUMENT_INDEX].lamports).borrow_mut();
        **lamports = (*lamports).saturating_add(5);
    }

    msg!("First invoke");
    let instruction = create_instruction(
        *accounts[INVOKED_PROGRAM_INDEX].key,
        &[
            (accounts[ARGUMENT_INDEX].key, true, true),
            (accounts[INVOKED_ARGUMENT_INDEX].key, true, true),
            (accounts[INVOKED_PROGRAM_INDEX].key, false, false),
        ],
        vec![NESTED_INVOKE, num_nested_invokes as u8],
    );
    invoke(&instruction, accounts)?;
    msg!("2nd invoke from first program");
    invoke(&instruction, accounts)?;

    assert_eq!(
        accounts[ARGUMENT_INDEX].lamports(),
        pre_argument_lamports
            .saturating_sub(5)
            .saturating_add(2_u64.saturating_mul(num_nested_invokes))
    );
    assert_eq!(
        accounts[INVOKED_ARGUMENT_INDEX].lamports(),
        pre_invoke_argument_lamports
            .saturating_add(5)
            .saturating_sub(2_u64.saturating_mul(num_nested_invokes))
    );
    Ok(())
}

solana_program::entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    msg!("invoke Rust program");

    let bump_seed1 = instruction_data[1];
    let bump_seed2 = instruction_data[2];
    let bump_seed3 = instruction_data[3];

    match instruction_data[0] {
        TEST_SUCCESS => {
            msg!("Call system program create account");
            {
                let from_lamports = accounts[FROM_INDEX].lamports();
                let to_lamports = accounts[DERIVED_KEY1_INDEX].lamports();
                assert_eq!(accounts[DERIVED_KEY1_INDEX].data_len(), 0);
                assert!(solana_program::system_program::check_id(
                    accounts[DERIVED_KEY1_INDEX].owner
                ));

                let instruction = system_instruction::create_account(
                    accounts[FROM_INDEX].key,
                    accounts[DERIVED_KEY1_INDEX].key,
                    42,
                    MAX_PERMITTED_DATA_INCREASE as u64,
                    program_id,
                );
                invoke_signed(
                    &instruction,
                    accounts,
                    &[&[b"You pass butter", &[bump_seed1]]],
                )?;

                assert_eq!(
                    accounts[FROM_INDEX].lamports(),
                    from_lamports.saturating_sub(42)
                );
                assert_eq!(
                    accounts[DERIVED_KEY1_INDEX].lamports(),
                    to_lamports.saturating_add(42)
                );
                assert_eq!(program_id, accounts[DERIVED_KEY1_INDEX].owner);
                assert_eq!(
                    accounts[DERIVED_KEY1_INDEX].data_len(),
                    MAX_PERMITTED_DATA_INCREASE
                );
                let mut data = accounts[DERIVED_KEY1_INDEX].try_borrow_mut_data()?;
                assert_eq!(data[MAX_PERMITTED_DATA_INCREASE.saturating_sub(1)], 0);
                data[MAX_PERMITTED_DATA_INCREASE.saturating_sub(1)] = 0x0f;
                assert_eq!(data[MAX_PERMITTED_DATA_INCREASE.saturating_sub(1)], 0x0f);
                for i in 0..20 {
                    data[i] = i as u8;
                }
            }

            msg!("Call system program transfer");
            {
                let from_lamports = accounts[FROM_INDEX].lamports();
                let to_lamports = accounts[DERIVED_KEY1_INDEX].lamports();
                let instruction = system_instruction::transfer(
                    accounts[FROM_INDEX].key,
                    accounts[DERIVED_KEY1_INDEX].key,
                    1,
                );
                invoke(&instruction, accounts)?;
                assert_eq!(
                    accounts[FROM_INDEX].lamports(),
                    from_lamports.saturating_sub(1)
                );
                assert_eq!(
                    accounts[DERIVED_KEY1_INDEX].lamports(),
                    to_lamports.saturating_add(1)
                );
            }

            msg!("Test data translation");
            {
                {
                    let mut data = accounts[ARGUMENT_INDEX].try_borrow_mut_data()?;
                    for i in 0..100 {
                        data[i as usize] = i;
                    }
                }

                let instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, true),
                        (accounts[INVOKED_ARGUMENT_INDEX].key, true, true),
                        (accounts[INVOKED_PROGRAM_INDEX].key, false, false),
                        (accounts[INVOKED_PROGRAM_DUP_INDEX].key, false, false),
                    ],
                    vec![VERIFY_TRANSLATIONS, 1, 2, 3, 4, 5],
                );
                invoke(&instruction, accounts)?;
            }

            msg!("Test no instruction data");
            {
                let instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[(accounts[ARGUMENT_INDEX].key, true, true)],
                    vec![],
                );
                invoke(&instruction, accounts)?;
            }

            msg!("Test refcell usage");
            {
                let writable = INVOKED_ARGUMENT_INDEX;
                let readable = INVOKED_PROGRAM_INDEX;

                let instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[
                        (accounts[writable].key, true, true),
                        (accounts[readable].key, false, false),
                    ],
                    vec![RETURN_OK, 1, 2, 3, 4, 5],
                );

                // success with this account configuration as a check
                invoke(&instruction, accounts)?;

                {
                    // writable but lamports borrow_mut'd
                    let _ref_mut = accounts[writable].try_borrow_mut_lamports()?;
                    assert_eq!(
                        invoke(&instruction, accounts),
                        Err(ProgramError::AccountBorrowFailed)
                    );
                }
                {
                    // writable but data borrow_mut'd
                    let _ref_mut = accounts[writable].try_borrow_mut_data()?;
                    assert_eq!(
                        invoke(&instruction, accounts),
                        Err(ProgramError::AccountBorrowFailed)
                    );
                }
                {
                    // writable but lamports borrow'd
                    let _ref_mut = accounts[writable].try_borrow_lamports()?;
                    assert_eq!(
                        invoke(&instruction, accounts),
                        Err(ProgramError::AccountBorrowFailed)
                    );
                }
                {
                    // writable but data borrow'd
                    let _ref_mut = accounts[writable].try_borrow_data()?;
                    assert_eq!(
                        invoke(&instruction, accounts),
                        Err(ProgramError::AccountBorrowFailed)
                    );
                }
                {
                    // readable but lamports borrow_mut'd
                    let _ref_mut = accounts[readable].try_borrow_mut_lamports()?;
                    assert_eq!(
                        invoke(&instruction, accounts),
                        Err(ProgramError::AccountBorrowFailed)
                    );
                }
                {
                    // readable but data borrow_mut'd
                    let _ref_mut = accounts[readable].try_borrow_mut_data()?;
                    assert_eq!(
                        invoke(&instruction, accounts),
                        Err(ProgramError::AccountBorrowFailed)
                    );
                }
                {
                    // readable but lamports borrow'd
                    let _ref_mut = accounts[readable].try_borrow_lamports()?;
                    invoke(&instruction, accounts)?;
                }
                {
                    // readable but data borrow'd
                    let _ref_mut = accounts[readable].try_borrow_data()?;
                    invoke(&instruction, accounts)?;
                }
            }

            msg!("Test create_program_address");
            {
                assert_eq!(
                    &Pubkey::create_program_address(
                        &[b"You pass butter", &[bump_seed1]],
                        program_id
                    )?,
                    accounts[DERIVED_KEY1_INDEX].key
                );
                let new_program_id = Pubkey::new_from_array([6u8; 32]);
                assert_eq!(
                    Pubkey::create_program_address(&[b"You pass butter"], &new_program_id)
                        .unwrap_err(),
                    PubkeyError::InvalidSeeds
                );
            }

            msg!("Test try_find_program_address");
            {
                let (address, bump_seed) =
                    Pubkey::try_find_program_address(&[b"You pass butter"], program_id).unwrap();
                assert_eq!(&address, accounts[DERIVED_KEY1_INDEX].key);
                assert_eq!(bump_seed, bump_seed1);
                let new_program_id = Pubkey::new_from_array([6u8; 32]);
                assert_eq!(
                    Pubkey::create_program_address(&[b"You pass butter"], &new_program_id)
                        .unwrap_err(),
                    PubkeyError::InvalidSeeds
                );
            }

            msg!("Test derived signers");
            {
                assert!(!accounts[DERIVED_KEY1_INDEX].is_signer);
                assert!(!accounts[DERIVED_KEY2_INDEX].is_signer);
                assert!(!accounts[DERIVED_KEY3_INDEX].is_signer);

                let invoked_instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[
                        (accounts[INVOKED_PROGRAM_INDEX].key, false, false),
                        (accounts[DERIVED_KEY1_INDEX].key, true, true),
                        (accounts[DERIVED_KEY2_INDEX].key, true, false),
                        (accounts[DERIVED_KEY3_INDEX].key, false, false),
                    ],
                    vec![DERIVED_SIGNERS, bump_seed2, bump_seed3],
                );
                invoke_signed(
                    &invoked_instruction,
                    accounts,
                    &[&[b"You pass butter", &[bump_seed1]]],
                )?;
            }

            msg!("Test readonly with writable account");
            {
                let invoked_instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[(accounts[ARGUMENT_INDEX].key, false, true)],
                    vec![VERIFY_WRITER],
                );
                invoke(&invoked_instruction, accounts)?;
            }

            msg!("Test nested invoke");
            {
                do_nested_invokes(4, accounts)?;
            }

            msg!("Test privilege deescalation");
            {
                assert!(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
                assert!(accounts[INVOKED_ARGUMENT_INDEX].is_writable);
                let invoked_instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[(accounts[INVOKED_ARGUMENT_INDEX].key, false, false)],
                    vec![VERIFY_PRIVILEGE_DEESCALATION],
                );
                invoke(&invoked_instruction, accounts)?;
            }

            msg!("Verify data values are retained and updated");
            {
                let data = accounts[ARGUMENT_INDEX].try_borrow_data()?;
                for i in 0..100 {
                    assert_eq!(data[i as usize], i);
                }
                let data = accounts[INVOKED_ARGUMENT_INDEX].try_borrow_data()?;
                for i in 0..10 {
                    assert_eq!(data[i as usize], i);
                }
            }

            msg!("Verify data write before cpi call with deescalated writable");
            {
                {
                    let mut data = accounts[ARGUMENT_INDEX].try_borrow_mut_data()?;
                    for i in 0..100 {
                        data[i as usize] = 42;
                    }
                }

                let invoked_instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[(accounts[ARGUMENT_INDEX].key, false, false)],
                    vec![VERIFY_PRIVILEGE_DEESCALATION],
                );
                invoke(&invoked_instruction, accounts)?;

                let data = accounts[ARGUMENT_INDEX].try_borrow_data()?;
                for i in 0..100 {
                    assert_eq!(data[i as usize], 42);
                }
            }

            msg!("Create account and init data");
            {
                let from_lamports = accounts[FROM_INDEX].lamports();
                let to_lamports = accounts[DERIVED_KEY2_INDEX].lamports();

                let instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[
                        (accounts[FROM_INDEX].key, true, true),
                        (accounts[DERIVED_KEY2_INDEX].key, true, false),
                        (accounts[SYSTEM_PROGRAM_INDEX].key, false, false),
                    ],
                    vec![CREATE_AND_INIT, bump_seed2],
                );
                invoke(&instruction, accounts)?;

                assert_eq!(
                    accounts[FROM_INDEX].lamports(),
                    from_lamports.saturating_sub(1)
                );
                assert_eq!(
                    accounts[DERIVED_KEY2_INDEX].lamports(),
                    to_lamports.saturating_add(1)
                );
                let data = accounts[DERIVED_KEY2_INDEX].try_borrow_mut_data()?;
                assert_eq!(data[0], 0x0e);
                assert_eq!(data[MAX_PERMITTED_DATA_INCREASE.saturating_sub(1)], 0x0f);
                for i in 1..20 {
                    assert_eq!(data[i], i as u8);
                }
            }

            msg!("Test return data via invoked");
            {
                // this should be cleared on entry, the invoked tests for this
                set_return_data(b"x");

                let instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[(accounts[ARGUMENT_INDEX].key, false, true)],
                    vec![SET_RETURN_DATA],
                );
                let _ = invoke(&instruction, accounts);

                assert_eq!(
                    get_return_data(),
                    Some((
                        *accounts[INVOKED_PROGRAM_INDEX].key,
                        b"Set by invoked".to_vec()
                    ))
                );
            }

            msg!("Test accounts re-ordering");
            {
                let instruction = create_instruction(
                    *accounts[INVOKED_PROGRAM_INDEX].key,
                    &[(accounts[FROM_INDEX].key, true, true)],
                    vec![RETURN_OK],
                );
                // put the relevant account at the end of a larger account list
                let mut reordered_accounts = accounts.to_vec();
                let account_info = reordered_accounts.remove(FROM_INDEX);
                reordered_accounts.push(accounts[0].clone());
                reordered_accounts.push(account_info);
                invoke(&instruction, &reordered_accounts)?;
            }
        }
        TEST_PRIVILEGE_ESCALATION_SIGNER => {
            msg!("Test privilege escalation signer");
            let mut invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[(accounts[DERIVED_KEY3_INDEX].key, false, false)],
                vec![VERIFY_PRIVILEGE_ESCALATION],
            );
            invoke(&invoked_instruction, accounts)?;

            // Signer privilege escalation will always fail the whole transaction
            invoked_instruction.accounts[0].is_signer = true;
            invoke(&invoked_instruction, accounts)?;
        }
        TEST_PRIVILEGE_ESCALATION_WRITABLE => {
            msg!("Test privilege escalation writable");
            let mut invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[(accounts[DERIVED_KEY3_INDEX].key, false, false)],
                vec![VERIFY_PRIVILEGE_ESCALATION],
            );
            invoke(&invoked_instruction, accounts)?;

            // Writable privilege escalation will always fail the whole transaction
            invoked_instruction.accounts[0].is_writable = true;
            invoke(&invoked_instruction, accounts)?;
        }
        TEST_PPROGRAM_NOT_EXECUTABLE => {
            msg!("Test program not executable");
            let instruction = create_instruction(
                *accounts[ARGUMENT_INDEX].key,
                &[(accounts[ARGUMENT_INDEX].key, true, true)],
                vec![RETURN_OK],
            );
            invoke(&instruction, accounts)?;
        }
        TEST_EMPTY_ACCOUNTS_SLICE => {
            msg!("Empty accounts slice");
            let instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[(accounts[INVOKED_ARGUMENT_INDEX].key, false, false)],
                vec![],
            );
            invoke(&instruction, &[])?;
        }
        TEST_CAP_SEEDS => {
            msg!("Test program max seeds");
            let instruction = create_instruction(*accounts[INVOKED_PROGRAM_INDEX].key, &[], vec![]);
            invoke_signed(
                &instruction,
                accounts,
                &[&[
                    b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9", b"0", b"1", b"2", b"3",
                    b"4", b"5", b"6", b"7",
                ]],
            )?;
        }
        TEST_CAP_SIGNERS => {
            msg!("Test program max signers");
            let instruction = create_instruction(*accounts[INVOKED_PROGRAM_INDEX].key, &[], vec![]);
            invoke_signed(
                &instruction,
                accounts,
                &[
                    &[b"1"],
                    &[b"2"],
                    &[b"3"],
                    &[b"4"],
                    &[b"5"],
                    &[b"6"],
                    &[b"7"],
                    &[b"8"],
                    &[b"9"],
                    &[b"0"],
                    &[b"1"],
                    &[b"2"],
                    &[b"3"],
                    &[b"4"],
                    &[b"5"],
                    &[b"6"],
                    &[b"7"],
                ],
            )?;
        }
        TEST_ALLOC_ACCESS_VIOLATION => {
            msg!("Test resize violation");
            let pubkey = *accounts[FROM_INDEX].key;
            let owner = *accounts[FROM_INDEX].owner;
            let ptr = accounts[FROM_INDEX].data.borrow().as_ptr() as u64 as *mut _;
            let len = accounts[FROM_INDEX].data_len();
            let data = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
            let mut lamports = accounts[FROM_INDEX].lamports();
            let from_info =
                AccountInfo::new(&pubkey, false, true, &mut lamports, data, &owner, false, 0);

            let pubkey = *accounts[DERIVED_KEY1_INDEX].key;
            let owner = *accounts[DERIVED_KEY1_INDEX].owner;
            // Point to top edge of heap, attempt to allocate into unprivileged memory
            let data = unsafe { std::slice::from_raw_parts_mut(0x300007ff8 as *mut _, 0) };
            let mut lamports = accounts[DERIVED_KEY1_INDEX].lamports();
            let derived_info =
                AccountInfo::new(&pubkey, false, true, &mut lamports, data, &owner, false, 0);

            let pubkey = *accounts[SYSTEM_PROGRAM_INDEX].key;
            let owner = *accounts[SYSTEM_PROGRAM_INDEX].owner;
            let ptr = accounts[SYSTEM_PROGRAM_INDEX].data.borrow().as_ptr() as u64 as *mut _;
            let len = accounts[SYSTEM_PROGRAM_INDEX].data_len();
            let data = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
            let mut lamports = accounts[SYSTEM_PROGRAM_INDEX].lamports();
            let system_info =
                AccountInfo::new(&pubkey, false, false, &mut lamports, data, &owner, true, 0);

            let instruction = system_instruction::create_account(
                accounts[FROM_INDEX].key,
                accounts[DERIVED_KEY1_INDEX].key,
                42,
                MAX_PERMITTED_DATA_INCREASE as u64,
                program_id,
            );

            invoke_signed(
                &instruction,
                &[system_info.clone(), from_info.clone(), derived_info.clone()],
                &[&[b"You pass butter", &[bump_seed1]]],
            )?;
        }
        TEST_MAX_INSTRUCTION_DATA_LEN_EXCEEDED => {
            msg!("Test max instruction data len exceeded");
            let data_len = MAX_CPI_INSTRUCTION_DATA_LEN.saturating_add(1) as usize;
            let instruction =
                create_instruction(*accounts[INVOKED_PROGRAM_INDEX].key, &[], vec![0; data_len]);
            invoke_signed(&instruction, &[], &[])?;
        }
        TEST_MAX_INSTRUCTION_ACCOUNTS_EXCEEDED => {
            msg!("Test max instruction accounts exceeded");
            let default_key = Pubkey::default();
            let account_metas_len = (MAX_CPI_INSTRUCTION_ACCOUNTS as usize).saturating_add(1);
            let account_metas = vec![(&default_key, false, false); account_metas_len];
            let instruction =
                create_instruction(*accounts[INVOKED_PROGRAM_INDEX].key, &account_metas, vec![]);
            invoke_signed(&instruction, &[], &[])?;
        }
        TEST_MAX_ACCOUNT_INFOS_EXCEEDED => {
            msg!("Test max account infos exceeded");
            let instruction = create_instruction(*accounts[INVOKED_PROGRAM_INDEX].key, &[], vec![]);
            let account_infos_len = MAX_CPI_ACCOUNT_INFOS.saturating_add(1);
            let account_infos = vec![accounts[0].clone(); account_infos_len];
            invoke_signed(&instruction, &account_infos, &[])?;
        }
        TEST_RETURN_ERROR => {
            msg!("Test return error");
            let instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[(accounts[INVOKED_ARGUMENT_INDEX].key, false, true)],
                vec![RETURN_ERROR],
            );
            let _ = invoke(&instruction, accounts);
        }
        TEST_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER => {
            msg!("Test privilege deescalation escalation signer");
            assert!(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
            assert!(accounts[INVOKED_ARGUMENT_INDEX].is_writable);
            let invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[
                    (accounts[INVOKED_PROGRAM_INDEX].key, false, false),
                    (accounts[INVOKED_ARGUMENT_INDEX].key, false, false),
                ],
                vec![VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER],
            );
            invoke(&invoked_instruction, accounts)?;
        }
        TEST_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE => {
            msg!("Test privilege deescalation escalation writable");
            assert!(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
            assert!(accounts[INVOKED_ARGUMENT_INDEX].is_writable);
            let invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[
                    (accounts[INVOKED_PROGRAM_INDEX].key, false, false),
                    (accounts[INVOKED_ARGUMENT_INDEX].key, false, false),
                ],
                vec![VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE],
            );
            invoke(&invoked_instruction, accounts)?;
        }
        TEST_WRITABLE_DEESCALATION_WRITABLE => {
            msg!("Test writable deescalation writable");
            const NUM_BYTES: usize = 10;
            let mut buffer = [0; NUM_BYTES];
            buffer.copy_from_slice(&accounts[INVOKED_ARGUMENT_INDEX].data.borrow()[..NUM_BYTES]);

            let instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[(accounts[INVOKED_ARGUMENT_INDEX].key, false, false)],
                vec![WRITE_ACCOUNT, NUM_BYTES as u8],
            );
            let _ = invoke(&instruction, accounts);

            assert_eq!(
                buffer,
                accounts[INVOKED_ARGUMENT_INDEX].data.borrow()[..NUM_BYTES]
            );
        }
        TEST_NESTED_INVOKE_TOO_DEEP => {
            let _ = do_nested_invokes(5, accounts);
        }
        TEST_CALL_PRECOMPILE => {
            msg!("Test calling precompiled program from cpi");
            let instruction =
                Instruction::new_with_bytes(*accounts[ED25519_PROGRAM_INDEX].key, &[], vec![]);
            invoke(&instruction, accounts)?;
        }
        ADD_LAMPORTS => {
            // make sure the total balance is fine
            {
                let mut lamports = (*accounts[0].lamports).borrow_mut();
                **lamports = (*lamports).saturating_add(1);
            }
        }
        TEST_RETURN_DATA_TOO_LARGE => {
            set_return_data(&[1u8; 1028]);
        }
        TEST_DUPLICATE_PRIVILEGE_ESCALATION_SIGNER => {
            msg!("Test duplicate privilege escalation signer");
            let mut invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[
                    (accounts[DERIVED_KEY3_INDEX].key, false, false),
                    (accounts[DERIVED_KEY3_INDEX].key, false, false),
                    (accounts[DERIVED_KEY3_INDEX].key, false, false),
                ],
                vec![VERIFY_PRIVILEGE_ESCALATION],
            );
            invoke(&invoked_instruction, accounts)?;

            // Signer privilege escalation will always fail the whole transaction
            invoked_instruction.accounts[1].is_signer = true;
            invoke(&invoked_instruction, accounts)?;
        }
        TEST_DUPLICATE_PRIVILEGE_ESCALATION_WRITABLE => {
            msg!("Test duplicate privilege escalation writable");
            let mut invoked_instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[
                    (accounts[DERIVED_KEY3_INDEX].key, false, false),
                    (accounts[DERIVED_KEY3_INDEX].key, false, false),
                    (accounts[DERIVED_KEY3_INDEX].key, false, false),
                ],
                vec![VERIFY_PRIVILEGE_ESCALATION],
            );
            invoke(&invoked_instruction, accounts)?;

            // Writable privilege escalation will always fail the whole transaction
            invoked_instruction.accounts[1].is_writable = true;
            invoke(&invoked_instruction, accounts)?;
        }
        TEST_FORBID_WRITE_AFTER_OWNERSHIP_CHANGE_IN_CALLEE => {
            msg!("TEST_FORBID_WRITE_AFTER_OWNERSHIP_CHANGE_IN_CALLEE");
            invoke(
                &create_instruction(
                    *program_id,
                    &[
                        (program_id, false, false),
                        (accounts[ARGUMENT_INDEX].key, true, false),
                    ],
                    vec![
                        TEST_FORBID_WRITE_AFTER_OWNERSHIP_CHANGE_IN_CALLEE_NESTED,
                        42,
                        42,
                        42,
                    ],
                ),
                accounts,
            )
            .unwrap();
            let account = &accounts[ARGUMENT_INDEX];
            // this should cause the tx to fail since the callee changed ownership
            unsafe {
                *account
                    .data
                    .borrow_mut()
                    .get_unchecked_mut(instruction_data[1] as usize) = 42
            };
        }
        TEST_FORBID_WRITE_AFTER_OWNERSHIP_CHANGE_IN_CALLEE_NESTED => {
            msg!("TEST_FORBID_WRITE_AFTER_OWNERSHIP_CHANGE_IN_CALLEE_NESTED");
            let account = &accounts[ARGUMENT_INDEX];
            account.data.borrow_mut().fill(0);
            account.assign(&system_program::id());
        }
        TEST_FORBID_WRITE_AFTER_OWNERSHIP_CHANGE_IN_CALLER => {
            msg!("TEST_FORBID_WRITE_AFTER_OWNERSHIP_CHANGE_IN_CALLER");
            let account = &accounts[ARGUMENT_INDEX];
            let invoked_program_id = accounts[INVOKED_PROGRAM_INDEX].key;
            account.data.borrow_mut().fill(0);
            account.assign(invoked_program_id);
            invoke(
                &create_instruction(
                    *invoked_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (invoked_program_id, false, false),
                    ],
                    vec![RETURN_OK],
                ),
                accounts,
            )
            .unwrap();
            // this should cause the tx to failsince invoked_program_id now owns
            // the account
            unsafe {
                *account
                    .data
                    .borrow_mut()
                    .get_unchecked_mut(instruction_data[1] as usize) = 42
            };
        }
        TEST_FORBID_LEN_UPDATE_AFTER_OWNERSHIP_CHANGE_MOVING_DATA_POINTER => {
            msg!("TEST_FORBID_LEN_UPDATE_AFTER_OWNERSHIP_CHANGE_MOVING_DATA_POINTER");
            const INVOKE_PROGRAM_INDEX: usize = 3;
            const REALLOC_PROGRAM_INDEX: usize = 4;
            let account = &accounts[ARGUMENT_INDEX];
            let realloc_program_id = accounts[REALLOC_PROGRAM_INDEX].key;
            let invoke_program_id = accounts[INVOKE_PROGRAM_INDEX].key;
            account.realloc(0, false).unwrap();
            account.assign(realloc_program_id);

            // Place a RcBox<RefCell<&mut [u8]>> in the account data. This
            // allows us to test having CallerAccount::ref_to_len_in_vm in an
            // account region.
            let rc_box_addr =
                account.data.borrow_mut().as_mut_ptr() as *mut RcBox<RefCell<&mut [u8]>>;
            let rc_box_size = mem::size_of::<RcBox<RefCell<&mut [u8]>>>();
            unsafe {
                std::ptr::write(
                    rc_box_addr,
                    RcBox {
                        strong: 1,
                        weak: 0,
                        // We're testing what happens if we make CPI update the
                        // slice length after we put the slice in the account
                        // address range. To do so, we need to move the data
                        // pointer past the RcBox or CPI will clobber the length
                        // change when it copies the callee's account data back
                        // into the caller's account data
                        // https://github.com/solana-labs/solana/blob/fa28958bd69054d1c2348e0a731011e93d44d7af/programs/bpf_loader/src/syscalls/cpi.rs#L1487
                        value: RefCell::new(slice::from_raw_parts_mut(
                            account.data.borrow_mut().as_mut_ptr().add(rc_box_size),
                            0,
                        )),
                    },
                );
            }

            // CPI now will update the serialized length in the wrong place,
            // since we moved the account data slice. To hit the corner case we
            // want to hit, we'll need to update the serialized length manually
            // or during deserialize_parameters() we'll get
            // AccountDataSizeChanged
            let serialized_len_ptr =
                unsafe { account.data.borrow_mut().as_mut_ptr().offset(-8) as *mut u64 };

            unsafe {
                overwrite_account_data(
                    account,
                    Rc::from_raw(((rc_box_addr as usize) + mem::size_of::<usize>() * 2) as *mut _),
                );
            }

            let mut instruction_data = vec![REALLOC, 0];
            instruction_data.extend_from_slice(&rc_box_size.to_le_bytes());

            // check that the account is empty before we CPI
            assert_eq!(account.data_len(), 0);

            invoke(
                &create_instruction(
                    *realloc_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (realloc_program_id, false, false),
                        (invoke_program_id, false, false),
                    ],
                    instruction_data.to_vec(),
                ),
                accounts,
            )
            .unwrap();

            // verify that CPI did update `ref_to_len_in_vm`
            assert_eq!(account.data_len(), rc_box_size);

            // update the serialized length so we don't error out early with AccountDataSizeChanged
            unsafe { *serialized_len_ptr = rc_box_size as u64 };

            // hack to avoid dropping the RcBox, which is supposed to be on the
            // heap but we put it into account data. If we don't do this,
            // dropping the Rc will cause
            // global_deallocator.dealloc(rc_box_addr) which is invalid and
            // happens to write a poison value into the account.
            unsafe {
                overwrite_account_data(account, Rc::new(RefCell::new(&mut [])));
            }
        }
        TEST_FORBID_LEN_UPDATE_AFTER_OWNERSHIP_CHANGE => {
            msg!("TEST_FORBID_LEN_UPDATE_AFTER_OWNERSHIP_CHANGE");
            const INVOKE_PROGRAM_INDEX: usize = 3;
            const REALLOC_PROGRAM_INDEX: usize = 4;
            let account = &accounts[ARGUMENT_INDEX];
            let target_account_index = instruction_data[1] as usize;
            let target_account = &accounts[target_account_index];
            let realloc_program_id = accounts[REALLOC_PROGRAM_INDEX].key;
            let invoke_program_id = accounts[INVOKE_PROGRAM_INDEX].key;
            account.realloc(0, false).unwrap();
            account.assign(realloc_program_id);
            target_account.realloc(0, false).unwrap();
            target_account.assign(realloc_program_id);

            let rc_box_addr =
                target_account.data.borrow_mut().as_mut_ptr() as *mut RcBox<RefCell<&mut [u8]>>;
            let rc_box_size = mem::size_of::<RcBox<RefCell<&mut [u8]>>>();
            unsafe {
                std::ptr::write(
                    rc_box_addr,
                    RcBox {
                        strong: 1,
                        weak: 0,
                        // The difference with
                        // TEST_FORBID_LEN_UPDATE_AFTER_OWNERSHIP_CHANGE_MOVING_DATA_POINTER
                        // is that we don't move the data pointer past the
                        // RcBox. This is needed to avoid the "Invalid account
                        // info pointer" check when direct mapping is enabled.
                        // This also means we don't need to update the
                        // serialized len like we do in the other test.
                        value: RefCell::new(slice::from_raw_parts_mut(
                            account.data.borrow_mut().as_mut_ptr(),
                            0,
                        )),
                    },
                );
            }

            let serialized_len_ptr =
                unsafe { account.data.borrow_mut().as_mut_ptr().offset(-8) as *mut u64 };
            // Place a RcBox<RefCell<&mut [u8]>> in the account data. This
            // allows us to test having CallerAccount::ref_to_len_in_vm in an
            // account region.
            unsafe {
                overwrite_account_data(
                    account,
                    Rc::from_raw(((rc_box_addr as usize) + mem::size_of::<usize>() * 2) as *mut _),
                );
            }

            let mut instruction_data = vec![REALLOC, 0];
            instruction_data.extend_from_slice(&rc_box_size.to_le_bytes());

            // check that the account is empty before we CPI
            assert_eq!(account.data_len(), 0);

            invoke(
                &create_instruction(
                    *realloc_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (target_account.key, true, false),
                        (realloc_program_id, false, false),
                        (invoke_program_id, false, false),
                    ],
                    instruction_data.to_vec(),
                ),
                accounts,
            )
            .unwrap();

            unsafe { *serialized_len_ptr = rc_box_size as u64 };
            // hack to avoid dropping the RcBox, which is supposed to be on the
            // heap but we put it into account data. If we don't do this,
            // dropping the Rc will cause
            // global_deallocator.dealloc(rc_box_addr) which is invalid and
            // happens to write a poison value into the account.
            unsafe {
                overwrite_account_data(account, Rc::new(RefCell::new(&mut [])));
            }
        }
        TEST_ALLOW_WRITE_AFTER_OWNERSHIP_CHANGE_TO_CALLER => {
            msg!("TEST_ALLOW_WRITE_AFTER_OWNERSHIP_CHANGE_TO_CALLER");
            const INVOKE_PROGRAM_INDEX: usize = 3;
            let account = &accounts[ARGUMENT_INDEX];
            let invoked_program_id = accounts[INVOKED_PROGRAM_INDEX].key;
            let invoke_program_id = accounts[INVOKE_PROGRAM_INDEX].key;
            invoke(
                &create_instruction(
                    *invoked_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (invoked_program_id, false, false),
                        (invoke_program_id, false, false),
                    ],
                    vec![ASSIGN_ACCOUNT_TO_CALLER],
                ),
                accounts,
            )
            .unwrap();
            // this should succeed since the callee gave us ownership of the
            // account
            unsafe {
                *account
                    .data
                    .borrow_mut()
                    .get_unchecked_mut(instruction_data[1] as usize) = 42
            };
        }
        TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS => {
            msg!("TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS");
            const CALLEE_PROGRAM_INDEX: usize = 3;
            let account = &accounts[ARGUMENT_INDEX];
            let callee_program_id = accounts[CALLEE_PROGRAM_INDEX].key;

            let expected = {
                let data = &instruction_data[1..];
                let prev_len = account.data_len();
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
        TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_NESTED => {
            msg!("TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_NESTED");
            const ARGUMENT_INDEX: usize = 0;
            let account = &accounts[ARGUMENT_INDEX];
            assert_eq!(*account.data.borrow(), &instruction_data[1..]);
        }
        TEST_CPI_ACCOUNT_UPDATE_CALLEE_GROWS => {
            msg!("TEST_CPI_ACCOUNT_UPDATE_CALLEE_GROWS");
            const REALLOC_PROGRAM_INDEX: usize = 2;
            const INVOKE_PROGRAM_INDEX: usize = 3;
            let account = &accounts[ARGUMENT_INDEX];
            let realloc_program_id = accounts[REALLOC_PROGRAM_INDEX].key;
            let realloc_program_owner = accounts[REALLOC_PROGRAM_INDEX].owner;
            let invoke_program_id = accounts[INVOKE_PROGRAM_INDEX].key;
            let mut instruction_data = instruction_data.to_vec();
            let mut expected = account.data.borrow().to_vec();
            expected.extend_from_slice(&instruction_data[1..]);
            instruction_data[0] = REALLOC_EXTEND_FROM_SLICE;
            invoke(
                &create_instruction(
                    *realloc_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (realloc_program_id, false, false),
                        (invoke_program_id, false, false),
                    ],
                    instruction_data.to_vec(),
                ),
                accounts,
            )
            .unwrap();

            if !bpf_loader_deprecated::check_id(realloc_program_owner) {
                assert_eq!(&*account.data.borrow(), &expected);
            }
        }
        TEST_CPI_ACCOUNT_UPDATE_CALLEE_SHRINKS_SMALLER_THAN_ORIGINAL_LEN => {
            msg!("TEST_CPI_ACCOUNT_UPDATE_CALLEE_SHRINKS_SMALLER_THAN_ORIGINAL_LEN");
            const REALLOC_PROGRAM_INDEX: usize = 2;
            const INVOKE_PROGRAM_INDEX: usize = 3;
            let account = &accounts[ARGUMENT_INDEX];
            let realloc_program_id = accounts[REALLOC_PROGRAM_INDEX].key;
            let realloc_program_owner = accounts[REALLOC_PROGRAM_INDEX].owner;
            let invoke_program_id = accounts[INVOKE_PROGRAM_INDEX].key;
            let new_len = usize::from_le_bytes(instruction_data[1..9].try_into().unwrap());
            let prev_len = account.data_len();
            let expected = account.data.borrow()[..new_len].to_vec();
            let mut instruction_data = vec![REALLOC, 0];
            instruction_data.extend_from_slice(&new_len.to_le_bytes());
            invoke(
                &create_instruction(
                    *realloc_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (realloc_program_id, false, false),
                        (invoke_program_id, false, false),
                    ],
                    instruction_data,
                ),
                accounts,
            )
            .unwrap();

            // deserialize_parameters_unaligned predates realloc support, and
            // hardcodes the account data length to the original length.
            if !bpf_loader_deprecated::check_id(realloc_program_owner) {
                assert_eq!(&*account.data.borrow(), &expected);
                assert_eq!(
                    unsafe {
                        slice::from_raw_parts(
                            account.data.borrow().as_ptr().add(new_len),
                            prev_len - new_len,
                        )
                    },
                    &vec![0; prev_len - new_len]
                );
            }
        }
        TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_CALLEE_SHRINKS => {
            msg!("TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_CALLEE_SHRINKS");
            const INVOKE_PROGRAM_INDEX: usize = 3;
            const SENTINEL: u8 = 42;
            let account = &accounts[ARGUMENT_INDEX];
            let invoke_program_id = accounts[INVOKE_PROGRAM_INDEX].key;

            let prev_data = {
                let data = &instruction_data[9..];
                let prev_len = account.data_len();
                account.realloc(prev_len + data.len(), false)?;
                account.data.borrow_mut()[prev_len..].copy_from_slice(data);
                unsafe {
                    // write a sentinel value just outside the account data to
                    // check that when CPI zeroes the realloc region it doesn't
                    // zero too much
                    *account
                        .data
                        .borrow_mut()
                        .as_mut_ptr()
                        .add(prev_len + data.len()) = SENTINEL;
                };
                account.data.borrow().to_vec()
            };

            let mut expected = account.data.borrow().to_vec();
            let new_len = usize::from_le_bytes(instruction_data[1..9].try_into().unwrap());
            expected.extend_from_slice(&instruction_data[9..]);
            let mut instruction_data =
                vec![TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_CALLEE_SHRINKS_NESTED];
            instruction_data.extend_from_slice(&new_len.to_le_bytes());
            invoke(
                &create_instruction(
                    *invoke_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (invoke_program_id, false, false),
                    ],
                    instruction_data,
                ),
                accounts,
            )
            .unwrap();

            assert_eq!(*account.data.borrow(), &prev_data[..new_len]);
            assert_eq!(
                unsafe {
                    slice::from_raw_parts(
                        account.data.borrow().as_ptr().add(new_len),
                        prev_data.len() - new_len,
                    )
                },
                &vec![0; prev_data.len() - new_len]
            );
            assert_eq!(
                unsafe { *account.data.borrow().as_ptr().add(prev_data.len()) },
                SENTINEL
            );
        }
        TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_CALLEE_SHRINKS_NESTED => {
            msg!("TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_CALLEE_SHRINKS_NESTED");
            const ARGUMENT_INDEX: usize = 0;
            let account = &accounts[ARGUMENT_INDEX];
            let new_len = usize::from_le_bytes(instruction_data[1..9].try_into().unwrap());
            account.realloc(new_len, false).unwrap();
        }
        TEST_CPI_INVALID_KEY_POINTER => {
            msg!("TEST_CPI_INVALID_KEY_POINTER");
            const CALLEE_PROGRAM_INDEX: usize = 2;
            let account = &accounts[ARGUMENT_INDEX];
            let key = *account.key;
            let key = &key as *const _ as usize;
            #[rustversion::attr(since(1.72), allow(invalid_reference_casting))]
            fn overwrite_account_key(account: &AccountInfo, key: *const Pubkey) {
                unsafe {
                    *mem::transmute::<_, *mut *const Pubkey>(&account.key) = key;
                }
            }
            overwrite_account_key(account, key as *const Pubkey);
            let callee_program_id = accounts[CALLEE_PROGRAM_INDEX].key;

            invoke(
                &create_instruction(
                    *callee_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (callee_program_id, false, false),
                    ],
                    vec![],
                ),
                accounts,
            )
            .unwrap();
        }
        TEST_CPI_INVALID_LAMPORTS_POINTER => {
            msg!("TEST_CPI_INVALID_LAMPORTS_POINTER");
            const CALLEE_PROGRAM_INDEX: usize = 2;
            let account = &accounts[ARGUMENT_INDEX];
            let mut lamports = account.lamports();
            account
                .lamports
                .replace(unsafe { mem::transmute(&mut lamports) });
            let callee_program_id = accounts[CALLEE_PROGRAM_INDEX].key;

            invoke(
                &create_instruction(
                    *callee_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (callee_program_id, false, false),
                    ],
                    vec![],
                ),
                accounts,
            )
            .unwrap();
        }
        TEST_CPI_INVALID_OWNER_POINTER => {
            msg!("TEST_CPI_INVALID_OWNER_POINTER");
            const CALLEE_PROGRAM_INDEX: usize = 2;
            let account = &accounts[ARGUMENT_INDEX];
            let owner = account.owner as *const _ as usize + 1;
            #[rustversion::attr(since(1.72), allow(invalid_reference_casting))]
            fn overwrite_account_owner(account: &AccountInfo, owner: *const Pubkey) {
                unsafe {
                    *mem::transmute::<_, *mut *const Pubkey>(&account.owner) = owner;
                }
            }
            overwrite_account_owner(account, owner as *const Pubkey);
            let callee_program_id = accounts[CALLEE_PROGRAM_INDEX].key;

            invoke(
                &create_instruction(
                    *callee_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (callee_program_id, false, false),
                    ],
                    vec![],
                ),
                accounts,
            )
            .unwrap();
        }
        TEST_CPI_INVALID_DATA_POINTER => {
            msg!("TEST_CPI_INVALID_DATA_POINTER");
            const CALLEE_PROGRAM_INDEX: usize = 2;
            let account = &accounts[ARGUMENT_INDEX];
            let data = unsafe {
                slice::from_raw_parts_mut(account.data.borrow_mut()[1..].as_mut_ptr(), 0)
            };
            account.data.replace(data);
            let callee_program_id = accounts[CALLEE_PROGRAM_INDEX].key;

            invoke(
                &create_instruction(
                    *callee_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (callee_program_id, false, false),
                    ],
                    vec![],
                ),
                accounts,
            )
            .unwrap();
        }
        TEST_CPI_CHANGE_ACCOUNT_DATA_MEMORY_ALLOCATION => {
            msg!("TEST_CPI_CHANGE_ACCOUNT_DATA_MEMORY_ALLOCATION");
            const CALLEE_PROGRAM_INDEX: usize = 2;
            let account = &accounts[ARGUMENT_INDEX];
            let callee_program_id = accounts[CALLEE_PROGRAM_INDEX].key;
            let original_data_len = account.data_len();

            // Initial data is all [0xFF; 20]
            assert_eq!(&*account.data.borrow(), &[0xFF; 20]);

            // Realloc to [0xFE; 10]
            invoke(
                &create_instruction(
                    *callee_program_id,
                    &[
                        (account.key, true, false),
                        (callee_program_id, false, false),
                    ],
                    vec![0xFE; 10],
                ),
                accounts,
            )
            .unwrap();

            // Check that [10..20] is zeroed
            let new_len = account.data_len();
            assert_eq!(&*account.data.borrow(), &[0xFE; 10]);
            assert_eq!(
                unsafe {
                    slice::from_raw_parts(
                        account.data.borrow().as_ptr().add(new_len),
                        original_data_len - new_len,
                    )
                },
                &vec![0; original_data_len - new_len]
            );

            // Realloc to [0xFD; 5]
            invoke(
                &create_instruction(
                    *callee_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (callee_program_id, false, false),
                    ],
                    vec![0xFD; 5],
                ),
                accounts,
            )
            .unwrap();

            // Check that [5..20] is zeroed
            let new_len = account.data_len();
            assert_eq!(&*account.data.borrow(), &[0xFD; 5]);
            assert_eq!(
                unsafe {
                    slice::from_raw_parts(
                        account.data.borrow().as_ptr().add(new_len),
                        original_data_len - new_len,
                    )
                },
                &vec![0; original_data_len - new_len]
            );

            // Realloc to [0xFC; 2]
            invoke(
                &create_instruction(
                    *callee_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (callee_program_id, false, false),
                    ],
                    vec![0xFC; 2],
                ),
                accounts,
            )
            .unwrap();

            // Check that [2..20] is zeroed
            let new_len = account.data_len();
            assert_eq!(&*account.data.borrow(), &[0xFC; 2]);
            assert_eq!(
                unsafe {
                    slice::from_raw_parts(
                        account.data.borrow().as_ptr().add(new_len),
                        original_data_len - new_len,
                    )
                },
                &vec![0; original_data_len - new_len]
            );

            // Realloc to [0xFC; 2]. Here we keep the same length, but realloc the underlying
            // vector. CPI must zero even if the length is unchanged.
            invoke(
                &create_instruction(
                    *callee_program_id,
                    &[
                        (accounts[ARGUMENT_INDEX].key, true, false),
                        (callee_program_id, false, false),
                    ],
                    vec![0xFC; 2],
                ),
                accounts,
            )
            .unwrap();

            // Check that [2..20] is zeroed
            let new_len = account.data_len();
            assert_eq!(&*account.data.borrow(), &[0xFC; 2]);
            assert_eq!(
                unsafe {
                    slice::from_raw_parts(
                        account.data.borrow().as_ptr().add(new_len),
                        original_data_len - new_len,
                    )
                },
                &vec![0; original_data_len - new_len]
            );
        }
        TEST_WRITE_ACCOUNT => {
            msg!("TEST_WRITE_ACCOUNT");
            let target_account_index = instruction_data[1] as usize;
            let target_account = &accounts[target_account_index];
            let byte_index = usize::from_le_bytes(instruction_data[2..10].try_into().unwrap());
            target_account.data.borrow_mut()[byte_index] = instruction_data[10];
        }
        _ => panic!("unexpected program data"),
    }

    Ok(())
}

#[repr(C)]
struct RcBox<T> {
    strong: usize,
    weak: usize,
    value: T,
}

#[rustversion::attr(since(1.72), allow(invalid_reference_casting))]
unsafe fn overwrite_account_data(account: &AccountInfo, data: Rc<RefCell<&mut [u8]>>) {
    std::ptr::write(
        &account.data as *const _ as usize as *mut Rc<RefCell<&mut [u8]>>,
        data,
    );
}
