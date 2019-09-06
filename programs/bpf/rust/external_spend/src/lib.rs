//! @brief Example Rust-based BPF program that moves a lamport from one account to another

extern crate solana_sdk;
use solana_sdk::entrypoint;
use solana_sdk::entrypoint::*;

entrypoint!(process_instruction);
fn process_instruction(ka: &mut [SolKeyedAccount], _info: &SolClusterInfo, _data: &[u8]) -> bool {
    // account 0 is the mint and not owned by this program, any debit of its lamports
    // should result in a failed program execution.  Test to ensure that this debit
    // is seen by the runtime and fails as expected
    *ka[0].lamports -= 1;

    true
}
