//! defines block cost related limits
//!

// see https://github.com/solana-labs/solana/issues/18944
pub const MAX_INSTRUMENT_COST: u64 = 200_000;
pub const MAX_NUMBER_BPF_INSTRUCTIONS_PER_BLOCK: u64 = 4_000;
pub const MAX_NUMBER_BPF_INSTRUCTIONS_PER_ACCOUNT: u64 = 200;

pub fn block_cost_max() -> u64 {
    MAX_NUMBER_BPF_INSTRUCTIONS_PER_BLOCK * MAX_INSTRUMENT_COST
}

pub fn account_cost_max() -> u64 {
    MAX_NUMBER_BPF_INSTRUCTIONS_PER_ACCOUNT * MAX_INSTRUMENT_COST
}

// 07-27-2021, compute_unit to microsecond conversion ratio collected from mainnet-beta
// differs between instructions. Some bpf instruction has much higher CU/US ratio
// (eg 7vxeyaXGLqcp66fFShqUdHxdacp4k4kwUpRSSeoZLCZ4 has average ratio 135), others
// have lower ratio (eg 9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin has an average ratio 14).
// With this, I am guestimating the flat_fee for sigver and account read/write
// as following. This can be adjusted when needed.
pub const SIGVER_COST: u64 = 1;
pub const NON_SIGNED_READONLY_ACCOUNT_ACCESS_COST: u64 = 1;
pub const NON_SIGNED_WRITABLE_ACCOUNT_ACCESS_COST: u64 = 2;
pub const SIGNED_READONLY_ACCOUNT_ACCESS_COST: u64 =
    SIGVER_COST + NON_SIGNED_READONLY_ACCOUNT_ACCESS_COST;
pub const SIGNED_WRITABLE_ACCOUNT_ACCESS_COST: u64 =
    SIGVER_COST + NON_SIGNED_WRITABLE_ACCOUNT_ACCESS_COST;
