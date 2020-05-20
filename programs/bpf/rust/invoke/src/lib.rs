//! @brief Example Rust-based BPF program that issues a cross-program-invocation

#![allow(unreachable_code)]

extern crate solana_sdk;

use solana_bpf_rust_invoked::instruction::*;
use solana_sdk::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    info,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_instruction,
};

// const MINT_INDEX: usize = 0;
const ARGUMENT_INDEX: usize = 1;
const INVOKED_PROGRAM_INDEX: usize = 2;
const INVOKED_ARGUMENT_INDEX: usize = 3;
const INVOKED_PROGRAM_DUP_INDEX: usize = 4;
// const ARGUMENT_DUP_INDEX: usize = 5;
const DERIVED_KEY1_INDEX: usize = 6;
const DERIVED_KEY2_INDEX: usize = 7;
const DERIVED_KEY3_INDEX: usize = 8;
// const SYSTEM_PROGRAM_INDEX: usize = 9;
const FROM_INDEX: usize = 10;

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    info!("invoke Rust program");

    info!("Call system program");
    {
        assert_eq!(accounts[FROM_INDEX].lamports(), 43);
        assert_eq!(accounts[ARGUMENT_INDEX].lamports(), 41);
        let instruction =
            system_instruction::transfer(accounts[FROM_INDEX].key, accounts[ARGUMENT_INDEX].key, 1);
        invoke(&instruction, accounts)?;
        assert_eq!(accounts[FROM_INDEX].lamports(), 42);
        assert_eq!(accounts[ARGUMENT_INDEX].lamports(), 42);
    }

    info!("Test data translation");
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
            vec![TEST_VERIFY_TRANSLATIONS, 1, 2, 3, 4, 5],
        );
        invoke(&instruction, accounts)?;
    }

    info!("Test return error");
    {
        let instruction = create_instruction(
            *accounts[INVOKED_PROGRAM_INDEX].key,
            &[(accounts[ARGUMENT_INDEX].key, true, true)],
            vec![TEST_RETURN_ERROR],
        );
        assert_eq!(
            invoke(&instruction, accounts),
            Err(ProgramError::Custom(42))
        );
    }

    info!("Test derived signers");
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
            vec![TEST_DERIVED_SIGNERS],
        );
        invoke_signed(
            &invoked_instruction,
            accounts,
            &[&["You pass butter"], &["Lil'", "Bits"]],
        )?;
    }

    info!("Test readonly with writable account");
    {
        let invoked_instruction = create_instruction(
            *accounts[INVOKED_PROGRAM_INDEX].key,
            &[(accounts[ARGUMENT_INDEX].key, false, true)],
            vec![TEST_VERIFY_WRITER],
        );
        invoke(&invoked_instruction, accounts)?;
    }

    info!("Test nested invoke");
    {
        assert!(accounts[ARGUMENT_INDEX].is_signer);

        **accounts[ARGUMENT_INDEX].lamports.borrow_mut() -= 5;
        **accounts[INVOKED_ARGUMENT_INDEX].lamports.borrow_mut() += 5;

        info!("Fist invoke");
        let instruction = create_instruction(
            *accounts[INVOKED_PROGRAM_INDEX].key,
            &[
                (accounts[ARGUMENT_INDEX].key, true, true),
                (accounts[INVOKED_ARGUMENT_INDEX].key, true, true),
                (accounts[INVOKED_PROGRAM_DUP_INDEX].key, false, false),
                (accounts[INVOKED_PROGRAM_DUP_INDEX].key, false, false),
            ],
            vec![TEST_NESTED_INVOKE],
        );
        invoke(&instruction, accounts)?;
        info!("2nd invoke from first program");
        invoke(&instruction, accounts)?;

        assert_eq!(accounts[ARGUMENT_INDEX].lamports(), 42 - 5 + 1 + 1 + 1 + 1);
        assert_eq!(
            accounts[INVOKED_ARGUMENT_INDEX].lamports(),
            10 + 5 - 1 - 1 - 1 - 1
        );
    }

    info!("Verify data values are retained and updated");
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

    Ok(())
}

solana_sdk_bpf_test::stubs!();
