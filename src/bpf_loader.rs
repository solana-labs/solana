//! BPF loader
use native_loader;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;

const BPF_LOADER_NAME: &str = "solana_bpf_loader";
const BPF_LOADER_PROGRAM_ID: [u8; 32] = [
    128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&BPF_LOADER_PROGRAM_ID)
}

pub fn account() -> Account {
    Account {
        tokens: 0,
        program_id: id(),
        userdata: BPF_LOADER_NAME.as_bytes().to_vec(),
        executable: true,
        loader_program_id: native_loader::id(),
    }
}
