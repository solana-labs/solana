//! defines block cost related limits
//!

// see https://github.com/solana-labs/solana/issues/18944
// and https://github.com/solana-labs/solana/pull/18994#issuecomment-896128992
//
pub const MAX_BLOCK_TIME_US: u64 = 400_000; // aiming at 400ms/block max time
pub const AVG_INSTRUCTION_TIME_US: u64 = 1_000; // average instruction execution time
pub const SYSTEM_PARALLELISM: u64 = 10;
pub const MAX_INSTRUCTION_COST: u64 = 200_000;
pub const MAX_NUMBER_BPF_INSTRUCTIONS_PER_ACCOUNT: u64 = 200;

// 4_000
pub const MAX_INSTRUCTIONS_PER_BLOCK: u64 =
    (MAX_BLOCK_TIME_US / AVG_INSTRUCTION_TIME_US) * SYSTEM_PARALLELISM;

// 8_000_000_000
pub const BLOCK_COST_MAX: u64 = MAX_INSTRUCTION_COST * MAX_INSTRUCTIONS_PER_BLOCK * 10;

// 800_000_000
pub const ACCOUNT_COST_MAX: u64 = MAX_INSTRUCTION_COST * MAX_INSTRUCTIONS_PER_BLOCK;

// 2_000
pub const COMPUTE_UNIT_TO_US_RATIO: u64 =
    (MAX_INSTRUCTION_COST / AVG_INSTRUCTION_TIME_US) * SYSTEM_PARALLELISM;

// signature takes average 10us, or 20K CU
pub const SIGNATURE_COST: u64 = COMPUTE_UNIT_TO_US_RATIO * 10;

// read account averages 5us, or 10K CU
pub const ACCOUNT_READ_COST: u64 = COMPUTE_UNIT_TO_US_RATIO * 5;

// write account averages 25us, or 50K CU
pub const ACCOUNT_WRITE_COST: u64 = COMPUTE_UNIT_TO_US_RATIO * 25;
