//! @brief Example Rust-based BPF program that prints out the parameters passed to it

#![cfg(not(test))]
#![no_std]

mod solana_sdk;

use solana_sdk::*;

struct SStruct {
    x: u64,
    y: u64,
    z: u64,
}

#[inline(never)]
fn return_sstruct() -> SStruct {
    SStruct { x: 1, y: 2, z: 3 }
}

fn process(ka: &mut [SolKeyedAccount], data: &[u8], info: &SolClusterInfo) -> bool {
    sol_log("Program identifier:");
    sol_log_key(&info.program_id);

    // Log the provided account keys and instruction input data.  In the case of
    // the no-op program, no account keys or input data are expected but real
    // programs will have specific requirements so they can do their work.
    sol_log("Account keys and instruction input data:");
    sol_log_params(ka, data);

    {
        // Test - use core methods, unwrap

        // valid bytes, in a stack-allocated array
        let sparkle_heart = [240, 159, 146, 150];

        let result_str = core::str::from_utf8(&sparkle_heart).unwrap();

        sol_log_64(0, 0, 0, 0, result_str.len() as u64);
        sol_log(result_str);
        assert_eq!("💖", result_str);
    }

    {
        // Test - struct return
        let s = return_sstruct();
        sol_log_64(0, 0, s.x, s.y, s.z);
        assert_eq!(s.x + s.y + s.z, 6);
    }

    sol_log("Success");
    true
}
