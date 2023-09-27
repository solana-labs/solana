//! Code related to partitioned rewards distribution
//!
use solana_sdk::clock::Slot;

#[allow(dead_code)]
#[derive(Debug)]
/// Configuration options for partitioned epoch rewards.
/// This struct allows various forms of testing, especially prior to feature activation.
pub struct PartitionedEpochRewardsConfig {
    /// Number of blocks for reward calculation and storing vote accounts.
    /// Distributing rewards to stake accounts begins AFTER this many blocks.
    /// Normally, this will be 1.
    /// if force_one_slot_partitioned_rewards, this will be 0 (ie. we take 0 blocks just for reward calculation)
    pub reward_calculation_num_blocks: Slot,
    /// number of stake accounts to store in one block during partitioned reward interval
    /// normally, this is a number tuned for reasonable performance, such as 4096 accounts/block
    /// if force_one_slot_partitioned_rewards, this will usually be u64::MAX so that all stake accounts are written in the first block
    pub stake_account_stores_per_block: Slot,
    /// if true, end of epoch bank rewards will force using partitioned rewards distribution.
    /// see `set_test_enable_partitioned_rewards`
    pub test_enable_partitioned_rewards: bool,
    /// if true, end of epoch non-partitioned bank rewards will test the partitioned rewards distribution vote and stake accounts
    /// This has a significant performance impact on the first slot in each new epoch.
    pub test_compare_partitioned_epoch_rewards: bool,
}

impl Default for PartitionedEpochRewardsConfig {
    fn default() -> Self {
        Self {
            // reward calculation happens synchronously during the first block of the epoch boundary.
            // So, # blocks for reward calculation is 1.
            reward_calculation_num_blocks: 1,
            // # stake accounts to store in one block during partitioned reward interval
            // Target to store 64 rewards per entry/tick in a block. A block has a minimum of 64
            // entries/tick. This gives 4096 total rewards to store in one block.
            // This constant affects consensus.
            stake_account_stores_per_block: 4096,
            test_enable_partitioned_rewards: false,
            test_compare_partitioned_epoch_rewards: false,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub enum TestPartitionedEpochRewards {
    #[default]
    None,
    CompareResults,
    ForcePartitionedEpochRewardsInOneBlock,
    PartitionedEpochRewardsConfigRewardBlocks {
        reward_calculation_num_blocks: u64,
        stake_account_stores_per_block: u64,
    },
}

#[allow(dead_code)]
impl PartitionedEpochRewardsConfig {
    pub fn new(test: TestPartitionedEpochRewards) -> Self {
        match test {
            TestPartitionedEpochRewards::None => Self::default(),
            TestPartitionedEpochRewards::CompareResults => {
                Self::set_test_compare_partitioned_epoch_rewards()
            }
            TestPartitionedEpochRewards::ForcePartitionedEpochRewardsInOneBlock => {
                Self::set_test_enable_partitioned_rewards()
            }
            TestPartitionedEpochRewards::PartitionedEpochRewardsConfigRewardBlocks {
                    reward_calculation_num_blocks, stake_account_stores_per_block } => {
                   Self::set_test_enable_partitioned_rewards_with_custom_number_of_stake_accounts_per_block(
                    reward_calculation_num_blocks,
                    stake_account_stores_per_block)
                }

        }
    }

    /// All rewards will be distributed in the first block in the epoch, matching
    /// consensus for the non-partitioned rewards, but running all the partitioned rewards
    /// code.
    fn set_test_enable_partitioned_rewards() -> Self {
        Self {
            reward_calculation_num_blocks: 0,
            stake_account_stores_per_block: u64::MAX,
            test_enable_partitioned_rewards: true,
            // irrelevant if we are not running old code path
            test_compare_partitioned_epoch_rewards: false,
        }
    }

    /// All rewards will be distributed in the first block in the epoch as normal.
    /// Then, the partitioned rewards code will calculate expected results and compare to
    /// the old code path's results.
    fn set_test_compare_partitioned_epoch_rewards() -> Self {
        Self {
            test_compare_partitioned_epoch_rewards: true,
            ..PartitionedEpochRewardsConfig::default()
        }
    }

    /// A method that configures how many reward reward calculation blocks and how many stake
    /// accounts to store per reward block.
    fn set_test_enable_partitioned_rewards_with_custom_number_of_stake_accounts_per_block(
        reward_calculation_num_blocks: u64,
        stake_account_stores_per_block: u64,
    ) -> Self {
        Self {
            reward_calculation_num_blocks,
            stake_account_stores_per_block,
            test_enable_partitioned_rewards: true,
            // irrelevant if we are not running old code path
            test_compare_partitioned_epoch_rewards: false,
        }
    }
}
