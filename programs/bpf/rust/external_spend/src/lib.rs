//! @brief Example Rust-based BPF program that moves a lamport from one account to another

extern crate solana_sdk_bpf;
use solana_sdk_bpf::account::Account;
use solana_sdk_bpf::entrypoint;
use solana_sdk_bpf::pubkey::Pubkey;

entrypoint!(process_instruction);
fn process_instruction(_program_id: &Pubkey, accounts: &mut [Account], _data: &[u8]) -> bool {
    // account 0 is the mint and not owned by this program, any debit of its lamports
    // should result in a failed program execution.  Test to ensure that this debit
    // is seen by the runtime and fails as expected
    *accounts[0].lamports -= 1;

    true
}
