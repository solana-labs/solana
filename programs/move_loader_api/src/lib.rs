pub mod account_state;
pub mod data_store;
pub mod error_mappers;
pub mod processor;

const MOVE_LOADER_PROGRAM_ID: [u8; 32] = [
    5, 84, 172, 160, 172, 5, 64, 41, 134, 4, 81, 31, 45, 11, 30, 64, 219, 238, 140, 38, 194, 100,
    192, 219, 156, 94, 62, 208, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    MOVE_LOADER_PROGRAM_ID,
    "MoveLdr111111111111111111111111111111111111"
);
