pub mod storage_contract;
pub mod storage_instruction;
pub mod storage_processor;

use solana_sdk::pubkey::Pubkey;

pub const SLOTS_PER_SEGMENT: u64 = 2;

pub fn get_segment_from_slot(slot: u64) -> usize {
    (slot / SLOTS_PER_SEGMENT) as usize
}

const STORAGE_PROGRAM_ID: [u8; 32] = [
    6, 162, 25, 123, 127, 68, 233, 59, 131, 151, 21, 152, 162, 120, 90, 37, 154, 88, 86, 5, 156,
    221, 182, 201, 142, 103, 151, 112, 0, 0, 0, 0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == STORAGE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&STORAGE_PROGRAM_ID)
}
