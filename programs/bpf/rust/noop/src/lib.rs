//! @brief Example Rust-based BPF program that prints out the parameters passed to it

#![allow(unreachable_code)]

extern crate solana_sdk;
use solana_sdk::entrypoint::*;
use solana_sdk::log::*;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::{entrypoint, info};

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

entrypoint!(process_instruction);
fn process_instruction(program_id: &Pubkey, ka: &mut [SolKeyedAccount], data: &[u8]) -> bool {
    info!("Program identifier:");
    program_id.log();

    // Log the provided account keys and instruction input data.  In the case of
    // the no-op program, no account keys or input data are expected but real
    // programs will have specific requirements so they can do their work.
    info!("Account keys and instruction input data:");
    sol_log_params(ka, data);

    {
        // Test - use std methods, unwrap

        // valid bytes, in a stack-allocated array
        let sparkle_heart = [240, 159, 146, 150];
        let result_str = std::str::from_utf8(&sparkle_heart).unwrap();
        assert_eq!(4, result_str.len());
        assert_eq!("💖", result_str);
        info!(result_str);
    }

    {
        // Test - struct return

        let s = return_sstruct();
        assert_eq!(s.x + s.y + s.z, 6);
    }

    {
        // Test - arch config
        #[cfg(not(target_arch = "bpf"))]
        panic!();
    }

    info!("Success");
    true
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_return_sstruct() {
        assert_eq!(SStruct { x: 1, y: 2, z: 3 }, return_sstruct());
    }
}
