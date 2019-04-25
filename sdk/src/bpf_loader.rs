use crate::pubkey::Pubkey;

const BPF_LOADER_PROGRAM_ID: [u8; 32] = [
    2, 168, 246, 145, 78, 136, 161, 107, 189, 35, 149, 133, 95, 100, 4, 217, 180, 244, 86, 183,
    130, 27, 176, 20, 87, 73, 66, 140, 0, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&BPF_LOADER_PROGRAM_ID)
}
