pub mod storage_contract;
pub mod storage_instruction;
pub mod storage_processor;

use solana_sdk::pubkey::Pubkey;

pub const ENTRIES_PER_SEGMENT: u64 = 16;

pub fn get_segment_from_entry(entry_height: u64) -> usize {
    (entry_height / ENTRIES_PER_SEGMENT) as usize
}

const STORAGE_PROGRAM_ID: [u8; 32] = [
    130, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == STORAGE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&STORAGE_PROGRAM_ID)
}
