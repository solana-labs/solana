//! Code related to partitioned rewards distribution
//!
use solana_sdk::clock::Slot;

#[derive(Debug)]
/// Configuration options for partitioned epoch rewards.
/// This struct allows various forms of testing, especially prior to feature activation.
pub(crate) struct PartitionedEpochRewardsConfig {
    /// Number of blocks for reward calculation and storing vote accounts.
    /// Distributing rewards to stake accounts begins AFTER this many blocks.
    pub(crate) reward_calculation_num_blocks: Slot,
    /// number of stake accounts to store in one block during partititioned reward interval
    /// if force_one_slot_partitioned_rewards, this will usually be 1
    pub(crate) stake_account_stores_per_block: Slot,
    /// if true, end of epoch bank rewards will force using partitioned rewards distribution
    pub(crate) force_use_partitioned_rewards: bool,
    /// if true, end of epoch non-partitioned bank rewards will test the partitioned rewards distribution vote and stake accounts
    /// This has a significant performance impact on the first slot in each new epoch.
    pub(crate) test_partitioned_epoch_rewards: bool,
}

impl Default for PartitionedEpochRewardsConfig {
    fn default() -> Self {
        Self {
            /// reward calculation happens synchronously during the first block of the epoch boundary.
            /// So, # blocks for reward calculation is 1.
            reward_calculation_num_blocks: 1,
            /// # stake accounts to store in one block during partitioned reward interval
            /// Target to store 64 rewards per entry/tick in a block. A block has a minimum of 64
            /// entries/tick. This gives 4096 total rewards to store in one block.
            /// This constant affects consensus.
            stake_account_stores_per_block: 4096,
            force_use_partitioned_rewards: false,
            test_partitioned_epoch_rewards: false,
        }
    }
}
