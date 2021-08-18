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

pub const fn max_instructions_per_block() -> u64 {
    (MAX_BLOCK_TIME_US / AVG_INSTRUCTION_TIME_US) * SYSTEM_PARALLELISM
}

pub const fn block_cost_max() -> u64 {
    MAX_INSTRUCTION_COST * max_instructions_per_block() * 10
}

pub const fn account_cost_max() -> u64 {
    MAX_INSTRUCTION_COST * max_instructions_per_block()
}

pub const fn compute_unit_to_us_ratio() -> u64 {
    (MAX_INSTRUCTION_COST / AVG_INSTRUCTION_TIME_US) * SYSTEM_PARALLELISM
}

pub const fn signature_cost() -> u64 {
    // signature takes average 10us
    compute_unit_to_us_ratio() * 10
}

pub const fn account_read_cost() -> u64 {
    // read account averages 5us
    compute_unit_to_us_ratio() * 5
}

pub const fn account_write_cost() -> u64 {
    // write account averages 25us
    compute_unit_to_us_ratio() * 25
}
