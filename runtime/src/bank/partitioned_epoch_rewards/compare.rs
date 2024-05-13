use {
    super::{Bank, PartitionedRewardsCalculation, PartitionedStakeReward},
    crate::bank::{RewardCalcTracer, RewardsMetrics, VoteReward},
    dashmap::DashMap,
    log::info,
    rayon::ThreadPool,
    solana_accounts_db::stake_rewards::StakeReward,
    solana_sdk::{
        account::{AccountSharedData, WritableAccount},
        clock::Epoch,
        pubkey::Pubkey,
        reward_info::RewardInfo,
    },
    std::collections::HashMap,
};

impl Bank {
    /// compare the vote and stake accounts between the normal rewards calculation code
    /// and the partitioned rewards calculation code
    /// `stake_rewards_expected` and `vote_rewards_expected` are the results of the normal rewards calculation code
    /// This fn should have NO side effects.
    pub(in crate::bank) fn compare_with_partitioned_rewards(
        &self,
        stake_rewards_expected: &[StakeReward],
        vote_rewards_expected: &DashMap<Pubkey, VoteReward>,
        rewarded_epoch: Epoch,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
    ) {
        let partitioned_rewards = self.calculate_rewards_for_partitioning(
            rewarded_epoch,
            reward_calc_tracer,
            thread_pool,
            &mut RewardsMetrics::default(),
        );
        Self::compare_with_partitioned_rewards_results(
            stake_rewards_expected,
            vote_rewards_expected,
            partitioned_rewards,
        );
    }

    /// compare the vote and stake accounts between the normal rewards calculation code
    /// and the partitioned rewards calculation code
    /// `stake_rewards_expected` and `vote_rewards_expected` are the results of the normal rewards calculation code
    /// This fn should have NO side effects.
    /// This fn is only called in tests or with a debug cli arg prior to partitioned rewards feature activation.
    fn compare_with_partitioned_rewards_results(
        stake_rewards_expected: &[StakeReward],
        vote_rewards_expected: &DashMap<Pubkey, VoteReward>,
        partitioned_rewards: PartitionedRewardsCalculation,
    ) {
        // put partitioned stake rewards in a hashmap
        let mut stake_rewards: HashMap<Pubkey, &PartitionedStakeReward> = HashMap::default();
        partitioned_rewards
            .stake_rewards_by_partition
            .stake_rewards_by_partition
            .iter()
            .flatten()
            .for_each(|stake_reward| {
                stake_rewards.insert(stake_reward.stake_pubkey, stake_reward);
            });

        // verify stake rewards match expected
        stake_rewards_expected.iter().for_each(|stake_reward| {
            let partitioned = stake_rewards.remove(&stake_reward.stake_pubkey).unwrap();
            let converted = PartitionedStakeReward::maybe_from(stake_reward).expect(
                "StakeRewards should only be deserializable accounts with state \
                 StakeStateV2::Stake",
            );
            assert_eq!(partitioned, &converted);
        });
        assert!(stake_rewards.is_empty(), "{stake_rewards:?}");

        let mut vote_rewards: HashMap<Pubkey, (RewardInfo, AccountSharedData)> = HashMap::default();
        partitioned_rewards
            .vote_account_rewards
            .accounts_to_store
            .iter()
            .enumerate()
            .for_each(|(i, account)| {
                if let Some(account) = account {
                    let reward = &partitioned_rewards.vote_account_rewards.rewards[i];
                    vote_rewards.insert(reward.0, (reward.1, account.clone()));
                }
            });

        // verify vote rewards match expected
        vote_rewards_expected.iter().for_each(|entry| {
            if entry.value().vote_needs_store {
                let partitioned = vote_rewards.remove(entry.key()).unwrap();
                let mut to_store_partitioned = partitioned.1.clone();
                to_store_partitioned.set_lamports(partitioned.0.post_balance);
                let mut to_store_normal = entry.value().vote_account.clone();
                _ = to_store_normal.checked_add_lamports(entry.value().vote_rewards);
                assert_eq!(to_store_partitioned, to_store_normal, "{:?}", entry.key());
            }
        });
        assert!(vote_rewards.is_empty(), "{vote_rewards:?}");
        info!(
            "verified partitioned rewards calculation matching: {}, {}",
            partitioned_rewards
                .stake_rewards_by_partition
                .stake_rewards_by_partition
                .iter()
                .map(|rewards| rewards.len())
                .sum::<usize>(),
            partitioned_rewards
                .vote_account_rewards
                .accounts_to_store
                .len()
        );
    }
}
