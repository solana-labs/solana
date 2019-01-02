//! @brief Example Rust-based BPF program that prints out the parameters passed to it

#![cfg(not(test))]
#![no_std]

mod solana_sdk;

use solana_sdk::*;

fn process(ka: &mut [SolKeyedAccount], data: &[u8], info: &SolClusterInfo) -> bool {
    sol_log("Tick height:");
    sol_log_64(info.tick_height, 0, 0, 0, 0);
    sol_log("Program identifier:");
    sol_log_key(&info.program_id);

    // Log the provided account keys and instruction input data.  In the case of
    // the no-op program, no account keys or input data are expected but real
    // programs will have specific requirements so they can do their work.
    sol_log("Account keys and instruction input data:");
    sol_log_params(ka, data);
    true
}
