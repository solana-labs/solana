//! Example Rust-based SBF program that issues a cross-program-invocation

#![cfg(feature = "program")]
#![allow(unreachable_code)]

use {
    crate::instructions::*,
    solana_program::{
        account_info::AccountInfo,
        entrypoint::{ProgramResult, MAX_PERMITTED_DATA_INCREASE},
        instruction::Instruction,
        msg,
        program::{get_return_data, invoke, invoke_signed, set_return_data},
        program_error::ProgramError,
        pubkey::{Pubkey, PubkeyError},
        syscalls::{
            MAX_CPI_ACCOUNT_INFOS, MAX_CPI_INSTRUCTION_ACCOUNTS, MAX_CPI_INSTRUCTION_DATA_LEN,
        },
        system_instruction,
    },
    solana_sbf_rust_invoked::instructions::*,
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
            buffer
                .copy_from_slice(&accounts[INVOKED_ARGUMENT_INDEX].data.borrow_mut()[..NUM_BYTES]);

            let instruction = create_instruction(
                *accounts[INVOKED_PROGRAM_INDEX].key,
                &[(accounts[INVOKED_ARGUMENT_INDEX].key, false, false)],
                vec![WRITE_ACCOUNT, NUM_BYTES as u8],
            );
            let _ = invoke(&instruction, accounts);

            assert_eq!(
                buffer,
                accounts[INVOKED_ARGUMENT_INDEX].data.borrow_mut()[..NUM_BYTES]
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
        _ => panic!(),
    }

    Ok(())
}
