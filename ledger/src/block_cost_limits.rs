//! defines block cost related limits
//!

// see https://github.com/solana-labs/solana/issues/18944
const MAX_INSTRUMENT_COST: u64 = 200_000;
const MAX_NUMBER_BPF_INSTRUCTIONS_PER_BLOCK: u64 = 3_399;

pub fn block_cost_max() -> u64 {
    MAX_NUMBER_BPF_INSTRUCTIONS_PER_BLOCK * MAX_INSTRUMENT_COST
}
