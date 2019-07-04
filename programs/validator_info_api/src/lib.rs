pub mod validator_info_instruction;
pub mod validator_info_processor;
pub mod validator_info_state;

const VALIDATOR_INFO_PROGRAM_ID: [u8; 32] = [
    7, 81, 151, 1, 116, 72, 242, 170, 104, 206, 95, 1, 65, 34, 195, 51, 197, 171, 251, 38, 235,
    101, 41, 114, 132, 224, 110, 100, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    VALIDATOR_INFO_PROGRAM_ID,
    "Va1idator1111111111111111111111111111111111"
);
