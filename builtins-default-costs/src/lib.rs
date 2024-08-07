#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]
use {
    ahash::AHashMap,
    lazy_static::lazy_static,
    solana_sdk::{
        address_lookup_table, bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
        compute_budget, ed25519_program, loader_v4, pubkey::Pubkey, secp256k1_program,
    },
};

// Number of compute units for each built-in programs
lazy_static! {
    /// Number of compute units for each built-in programs
    ///
    /// DEVELOPER WARNING: This map CANNOT be modified without causing a
    /// consensus failure because this map is used to calculate the compute
    /// limit for transactions that don't specify a compute limit themselves as
    /// of https://github.com/anza-xyz/agave/issues/2212.  It's also used to
    /// calculate the cost of a transaction which is used in replay to enforce
    /// block cost limits as of
    /// https://github.com/solana-labs/solana/issues/29595.
    pub static ref BUILTIN_INSTRUCTION_COSTS: AHashMap<Pubkey, u64> = [
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
        (secp256k1_program::id(), 0),
        (ed25519_program::id(), 0),
        // DO NOT ADD MORE ENTRIES TO THIS MAP
    ]
    .iter()
    .cloned()
    .collect();
}
