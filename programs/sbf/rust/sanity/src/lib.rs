//! Example Rust-based SBF sanity program that prints out the parameters passed to it

#![allow(unreachable_code)]
#![allow(clippy::arithmetic_side_effects)]

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo, bpf_loader, entrypoint::ProgramResult, log::*, msg,
    program::check_type_assumptions, pubkey::Pubkey,
};

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

solana_program::entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    msg!("Program identifier:");
    program_id.log();

    assert!(!bpf_loader::check_id(program_id));

    // Log the provided account keys and instruction input data.  In the case of
    // the no-op program, no account keys or input data are expected but real
    // programs will have specific requirements so they can do their work.
    msg!("Account keys and instruction input data:");
    sol_log_params(accounts, instruction_data);

    {
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

    {
        // Test - float math functions
        let zero = accounts[0].try_borrow_mut_data()?.len() as f64;
        let num = zero + 8.0f64;
        let num = num.powf(0.333f64);
        // check that the result is in a correct interval close to 1.998614185980905
        assert!(1.9986f64 < num && num < 2.0f64);
    }

    check_type_assumptions();

    sol_log_compute_units();
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_return_sstruct() {
        assert_eq!(SStruct { x: 1, y: 2, z: 3 }, return_sstruct());
    }
}
