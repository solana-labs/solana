//! defines block cost related limits
//!
use {
    lazy_static::lazy_static,
    solana_sdk::{
        address_lookup_table, bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
        compute_budget, ed25519_program, loader_v4, pubkey::Pubkey, secp256k1_program,
    },
    std::collections::HashMap,
};

/// Static configurations:
///
/// Number of microseconds replaying a block should take, 400 millisecond block times
/// is currently publicly communicated on solana.com
pub const MAX_BLOCK_REPLAY_TIME_US: u64 = 400_000;
/// number of concurrent processes,
pub const MAX_CONCURRENCY: u64 = 4;

// Cluster data, method of collecting at https://github.com/solana-labs/solana/issues/19627
// Dashboard: https://metrics.solana.com/d/monitor-edge/cluster-telemetry?orgId=1

/// Cluster averaged compute unit to micro-sec conversion rate
pub const COMPUTE_UNIT_TO_US_RATIO: u64 = 30;
/// Number of compute units for one signature verification.
pub const SIGNATURE_COST: u64 = COMPUTE_UNIT_TO_US_RATIO * 24;
/// Number of compute units for one write lock
pub const WRITE_LOCK_UNITS: u64 = COMPUTE_UNIT_TO_US_RATIO * 10;
/// Number of data bytes per compute units
pub const INSTRUCTION_DATA_BYTES_COST: u64 = 140 /*bytes per us*/ / COMPUTE_UNIT_TO_US_RATIO;
// Number of compute units for each built-in programs
lazy_static! {
    /// Number of compute units for each built-in programs
    pub static ref BUILT_IN_INSTRUCTION_COSTS: HashMap<Pubkey, u64> = [
        (solana_stake_program::id(), solana_stake_program::stake_instruction::DEFAULT_COMPUTE_UNITS),
        (solana_config_program::id(), solana_config_program::config_processor::DEFAULT_COMPUTE_UNITS),
        (solana_vote_program::id(), solana_vote_program::vote_processor::DEFAULT_COMPUTE_UNITS),
        (solana_system_program::id(), solana_system_program::system_processor::DEFAULT_COMPUTE_UNITS),
        (compute_budget::id(), solana_compute_budget_program::DEFAULT_COMPUTE_UNITS),
        (address_lookup_table::program::id(), solana_address_lookup_table_program::processor::DEFAULT_COMPUTE_UNITS),
        (bpf_loader_upgradeable::id(), solana_bpf_loader_program::UPGRADEABLE_LOADER_COMPUTE_UNITS),
        (bpf_loader_deprecated::id(), solana_bpf_loader_program::DEPRECATED_LOADER_COMPUTE_UNITS),
        (bpf_loader::id(), solana_bpf_loader_program::DEFAULT_LOADER_COMPUTE_UNITS),
        (loader_v4::id(), solana_loader_v4_program::DEFAULT_COMPUTE_UNITS),
        // Note: These are precompile, run directly in bank during sanitizing;
        (secp256k1_program::id(), COMPUTE_UNIT_TO_US_RATIO * 24),
        (ed25519_program::id(), COMPUTE_UNIT_TO_US_RATIO * 24),
    ]
    .iter()
    .cloned()
    .collect();
}

/// Statically computed data:
///
/// Number of compute units that a block is allowed. A block's compute units are
/// accumulated by Transactions added to it; A transaction's compute units are
/// calculated by cost_model, based on transaction's signatures, write locks,
/// data size and built-in and SBF instructions.
pub const MAX_BLOCK_UNITS: u64 =
    MAX_BLOCK_REPLAY_TIME_US * COMPUTE_UNIT_TO_US_RATIO * MAX_CONCURRENCY;

#[cfg(test)]
static_assertions::const_assert_eq!(MAX_BLOCK_UNITS, 48_000_000);

/// Number of compute units that a writable account in a block is allowed. The
/// limit is to prevent too many transactions write to same account, therefore
/// reduce block's parallelism.
pub const MAX_WRITABLE_ACCOUNT_UNITS: u64 = MAX_BLOCK_REPLAY_TIME_US * COMPUTE_UNIT_TO_US_RATIO;

#[cfg(test)]
static_assertions::const_assert_eq!(MAX_WRITABLE_ACCOUNT_UNITS, 12_000_000);

/// Number of compute units that a block can have for vote transactions,
/// sets at ~75% of MAX_BLOCK_UNITS to leave room for non-vote transactions
pub const MAX_VOTE_UNITS: u64 = (MAX_BLOCK_UNITS as f64 * 0.75_f64) as u64;

#[cfg(test)]
static_assertions::const_assert_eq!(MAX_VOTE_UNITS, 36_000_000);

/// The maximum allowed size, in bytes, that accounts data can grow, per block.
/// This can also be thought of as the maximum size of new allocations per block.
pub const MAX_BLOCK_ACCOUNTS_DATA_SIZE_DELTA: u64 = 100_000_000;
