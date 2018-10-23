//! BPF loader
use native_loader;
use solana_program_interface::account::Account;
use solana_program_interface::pubkey::Pubkey;

pub const BPF_LOADER_PROGRAM_ID: [u8; 32] = [6u8; 32];
pub const BPF_LOADER_NAME: &str = "bpf_loader";

pub fn id() -> Pubkey {
    Pubkey::new(&BPF_LOADER_PROGRAM_ID)
}

pub fn populate_account(account: &mut Account) {
    account.tokens = 0;
    account.program_id = id();
    account.userdata = BPF_LOADER_NAME.as_bytes().to_vec();
    account.executable = true;
    account.loader_program_id = native_loader::id();
}
