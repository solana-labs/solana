//! sanitized (versioned) transaction meta data.
//!
use crate::{
    entrypoint::HEAP_LENGTH as MIN_HEAP_FRAME_BYTES,
};

// TODO - remove the dup in program-runtime::compute_budget.rs
pub const MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT: u32 = 200_000;
pub const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;
pub const MAX_HEAP_FRAME_BYTES: usize = 256 * 1024;

// Contains transaction meta data, currently, set up compute_budget instructions
// to commit or request "resource" allocation for transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionMeta {
    pub updated_heap_bytes: usize,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
    /// Maximum accounts data size, in bytes, that a transaction is allowed to load; The
    /// value is capped by MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES to prevent overuse of memory.
    pub accounts_loaded_bytes: usize,
    // NOTE -
    // is_simple_vote can be consolidated into Meta here.
    //
    // other potential compute_budget ix or TLV ix related data here:
    //    pub accounts_written_bytes: usize,
    //    pub storage_bytes: usize,
    //    pub network_usage_bytes: usize,
    // below are meta data _after_ all accounts are loaded
    //    pub durable_nonce_account: Pubkey,
    //    pub cost_basis: something of cost_model,
}

impl Default for TransactionMeta {
    fn default() -> Self {
        TransactionMeta {
            updated_heap_bytes: MIN_HEAP_FRAME_BYTES,
            compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
            compute_unit_price: 0,
            accounts_loaded_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
        }
    }
}
