use crate::pubkey::Pubkey;

const BPF_LOADER_PROGRAM_ID: [u8; 32] = [
    128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&BPF_LOADER_PROGRAM_ID)
}
