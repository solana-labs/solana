//! @brief Example Rust-based BPF program that issues a cross-program-invocation

#![allow(unreachable_code)]

extern crate solana_sdk;

use solana_bpf_rust_invoked::instruction::create_instruction;
use solana_sdk::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    info,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
};

// const MINT_INDEX: usize = 0;
const ARGUMENT_INDEX: usize = 1;
const INVOKED_PROGRAM_INDEX: usize = 2;
const INVOKED_ARGUMENT_INDEX: usize = 3;
const INVOKED_PROGRAM_DUP_INDEX: usize = 4;
// const ARGUMENT_DUP_INDEX: usize = 5;
const DERIVED_KEY_INDEX: usize = 6;
const DERIVED_KEY2_INDEX: usize = 7;

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    info!("invoke Rust program");

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
            vec![0, 1, 2, 3, 4, 5],
        );
        invoke(&instruction, accounts)?;
    }

    info!("Test return error");
    {
        let instruction = create_instruction(
            *accounts[INVOKED_PROGRAM_INDEX].key,
            &[(accounts[ARGUMENT_INDEX].key, true, true)],
            vec![1],
        );
        assert_eq!(
            invoke(&instruction, accounts),
            Err(ProgramError::Custom(42))
        );
    }

    info!("Test derived signers");
    {
        assert!(!accounts[DERIVED_KEY_INDEX].is_signer);
        assert!(!accounts[DERIVED_KEY2_INDEX].is_signer);

        let invoked_instruction = create_instruction(
            *accounts[INVOKED_PROGRAM_INDEX].key,
            &[
                (accounts[DERIVED_KEY_INDEX].key, true, true),
                (accounts[DERIVED_KEY2_INDEX].key, false, true),
            ],
            vec![2],
        );
        invoke_signed(
            &invoked_instruction,
            accounts,
            &[&["Lil'", "Bits"], &["Gar Ma Nar Nar"]],
        )?;
    }

    info!("Test readonly with writable account");
    {
        let invoked_instruction = create_instruction(
            *accounts[INVOKED_PROGRAM_INDEX].key,
            &[(accounts[ARGUMENT_INDEX].key, false, true)],
            vec![3],
        );
        invoke(&invoked_instruction, accounts)?;
    }

    info!("Test nested invoke");
    {
        assert!(accounts[ARGUMENT_INDEX].is_signer);
        assert!(!accounts[DERIVED_KEY_INDEX].is_signer);
        assert!(!accounts[DERIVED_KEY2_INDEX].is_signer);

        **accounts[ARGUMENT_INDEX].lamports.borrow_mut() -= 5;
        **accounts[INVOKED_ARGUMENT_INDEX].lamports.borrow_mut() += 5;

        info!("Fist invoke");
        let instruction = create_instruction(
            *accounts[INVOKED_PROGRAM_INDEX].key,
            &[
                (accounts[ARGUMENT_INDEX].key, true, true),
                (accounts[INVOKED_ARGUMENT_INDEX].key, true, true),
                (accounts[DERIVED_KEY_INDEX].key, true, false),
                (accounts[INVOKED_PROGRAM_DUP_INDEX].key, false, false),
                (accounts[INVOKED_PROGRAM_DUP_INDEX].key, false, false),
            ],
            vec![4],
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
