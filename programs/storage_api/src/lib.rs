pub mod rewards_pools;
pub mod storage_contract;
pub mod storage_instruction;
pub mod storage_processor;

const STORAGE_PROGRAM_ID: [u8; 32] = [
    6, 162, 25, 123, 127, 68, 233, 59, 131, 151, 21, 152, 162, 120, 90, 37, 154, 88, 86, 5, 156,
    221, 182, 201, 142, 103, 151, 112, 0, 0, 0, 0,
];
solana_sdk::solana_name_id!(
    STORAGE_PROGRAM_ID,
    "Storage111111111111111111111111111111111111"
);
