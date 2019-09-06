//! @brief Example Rust-based BPF program that moves a lamport from one account to another

extern crate solana_sdk_bpf_utils;
use solana_sdk_bpf_utils::entrypoint;
use solana_sdk_bpf_utils::entrypoint::*;

entrypoint!(process_instruction);
<<<<<<< HEAD
fn process_instruction(ka: &mut [SolKeyedAccount], _info: &SolClusterInfo, _data: &[u8]) -> bool {
=======
fn process_instruction(_program_id: &Pubkey, ka: &mut [SolKeyedAccount], _data: &[u8]) -> u32 {
>>>>>>> 81c36699c...  Add support for BPF program custom errors (#5743)
    // account 0 is the mint and not owned by this program, any debit of its lamports
    // should result in a failed program execution.  Test to ensure that this debit
    // is seen by the runtime and fails as expected
    *ka[0].lamports -= 1;
    SUCCESS
}
