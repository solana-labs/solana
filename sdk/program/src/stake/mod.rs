pub mod config;
pub mod instruction;
pub mod state;

pub mod program {
    crate::declare_id!("Stake11111111111111111111111111111111111111");
}

/// The minimum stake amount that can be delegated, in lamports.
/// NOTE: This is also used to calculate the minimum balance of a stake account, which is the
/// rent exempt reserve _plus_ the minimum stake delegation.
pub const MINIMUM_STAKE_DELEGATION: u64 = 1;
