//! A type to hold data for the [`EpochRewards` sysvar][sv].
//!
//! [sv]: https://docs.solana.com/developing/runtime-facilities/sysvars#EpochRewards
//!
//! The sysvar ID is declared in [`sysvar::epoch_rewards`].
//!
//! [`sysvar::epoch_rewards`]: crate::sysvar::epoch_rewards

use std::ops::AddAssign;
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone, Copy, AbiExample)]
pub struct EpochRewards {
    /// total rewards for the current epoch, in lamports
    pub total_rewards: u64,

    /// distributed rewards for the current epoch, in lamports
    pub distributed_rewards: u64,

    /// distribution of all staking rewards for the current
    /// epoch will be completed at this block height
    pub distribution_complete_block_height: u64,
}

impl EpochRewards {
    pub fn distribute(&mut self, amount: u64) {
        assert!(self.distributed_rewards.saturating_add(amount) <= self.total_rewards);

        self.distributed_rewards.add_assign(amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl EpochRewards {
        pub fn new(
            total_rewards: u64,
            distributed_rewards: u64,
            distribution_complete_block_height: u64,
        ) -> Self {
            Self {
                total_rewards,
                distributed_rewards,
                distribution_complete_block_height,
            }
        }
    }

    #[test]
    fn test_epoch_rewards_new() {
        let epoch_rewards = EpochRewards::new(100, 0, 64);

        assert_eq!(epoch_rewards.total_rewards, 100);
        assert_eq!(epoch_rewards.distributed_rewards, 0);
        assert_eq!(epoch_rewards.distribution_complete_block_height, 64);
    }

    #[test]
    fn test_epoch_rewards_distribute() {
        let mut epoch_rewards = EpochRewards::new(100, 0, 64);
        epoch_rewards.distribute(100);

        assert_eq!(epoch_rewards.total_rewards, 100);
        assert_eq!(epoch_rewards.distributed_rewards, 100);
    }

    #[test]
    #[should_panic(
        expected = "self.distributed_rewards.saturating_add(amount) <= self.total_rewards"
    )]
    fn test_epoch_rewards_distribute_panic() {
        let mut epoch_rewards = EpochRewards::new(100, 0, 64);
        epoch_rewards.distribute(200);
    }
}
