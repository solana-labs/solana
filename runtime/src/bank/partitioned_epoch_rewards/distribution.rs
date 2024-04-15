use {
    super::{Bank, EpochRewardStatus},
    crate::bank::metrics::{report_partitioned_reward_metrics, RewardsStoreMetrics},
    solana_accounts_db::stake_rewards::StakeReward,
    solana_measure::measure_us,
    solana_sdk::account::ReadableAccount,
    std::sync::atomic::Ordering::Relaxed,
};

impl Bank {
    /// Process reward distribution for the block if it is inside reward interval.
    pub(in crate::bank) fn distribute_partitioned_epoch_rewards(&mut self) {
        let EpochRewardStatus::Active(status) = &self.epoch_reward_status else {
            return;
        };

        let height = self.block_height();
        let start_block_height = status.start_block_height;
        let credit_start = start_block_height + self.get_reward_calculation_num_blocks();
        let credit_end_exclusive = credit_start + status.stake_rewards_by_partition.len() as u64;
        assert!(
            self.epoch_schedule.get_slots_in_epoch(self.epoch)
                > credit_end_exclusive.saturating_sub(credit_start)
        );

        if height >= credit_start && height < credit_end_exclusive {
            let partition_index = height - credit_start;
            self.distribute_epoch_rewards_in_partition(
                &status.stake_rewards_by_partition,
                partition_index,
            );
        }

        if height.saturating_add(1) >= credit_end_exclusive {
            datapoint_info!(
                "epoch-rewards-status-update",
                ("slot", self.slot(), i64),
                ("block_height", height, i64),
                ("active", 0, i64),
                ("start_block_height", start_block_height, i64),
            );

            assert!(matches!(
                self.epoch_reward_status,
                EpochRewardStatus::Active(_)
            ));
            self.epoch_reward_status = EpochRewardStatus::Inactive;
            self.set_epoch_rewards_sysvar_to_inactive();
        }
    }

    /// Process reward credits for a partition of rewards
    /// Store the rewards to AccountsDB, update reward history record and total capitalization.
    fn distribute_epoch_rewards_in_partition(
        &self,
        all_stake_rewards: &[Vec<StakeReward>],
        partition_index: u64,
    ) {
        let pre_capitalization = self.capitalization();
        let this_partition_stake_rewards = &all_stake_rewards[partition_index as usize];

        let (total_rewards_in_lamports, store_stake_accounts_us) =
            measure_us!(self.store_stake_accounts_in_partition(this_partition_stake_rewards));

        // increase total capitalization by the distributed rewards
        self.capitalization
            .fetch_add(total_rewards_in_lamports, Relaxed);

        // decrease distributed capital from epoch rewards sysvar
        self.update_epoch_rewards_sysvar(total_rewards_in_lamports);

        // update reward history for this partitioned distribution
        self.update_reward_history_in_partition(this_partition_stake_rewards);

        let metrics = RewardsStoreMetrics {
            pre_capitalization,
            post_capitalization: self.capitalization(),
            total_stake_accounts_count: all_stake_rewards.len(),
            partition_index,
            store_stake_accounts_us,
            store_stake_accounts_count: this_partition_stake_rewards.len(),
            distributed_rewards: total_rewards_in_lamports,
        };

        report_partitioned_reward_metrics(self, metrics);
    }

    /// insert non-zero stake rewards to self.rewards
    /// Return the number of rewards inserted
    fn update_reward_history_in_partition(&self, stake_rewards: &[StakeReward]) -> usize {
        let mut rewards = self.rewards.write().unwrap();
        rewards.reserve(stake_rewards.len());
        let initial_len = rewards.len();
        stake_rewards
            .iter()
            .filter(|x| x.get_stake_reward() > 0)
            .for_each(|x| rewards.push((x.stake_pubkey, x.stake_reward_info)));
        rewards.len().saturating_sub(initial_len)
    }

    /// store stake rewards in partition
    /// return the sum of all the stored rewards
    ///
    /// Note: even if staker's reward is 0, the stake account still needs to be stored because
    /// credits observed has changed
    fn store_stake_accounts_in_partition(&self, stake_rewards: &[StakeReward]) -> u64 {
        // Verify that stake account `lamports + reward_amount` matches what we have in the
        // rewarded account. This code will have a performance hit - an extra load and compare of
        // the stake accounts. This is for debugging. Once we are confident, we can disable the
        // check.
        const VERIFY_REWARD_LAMPORT: bool = true;

        if VERIFY_REWARD_LAMPORT {
            for r in stake_rewards {
                let stake_pubkey = r.stake_pubkey;
                let reward_amount = r.get_stake_reward();
                let post_stake_account = &r.stake_account;
                if let Some(curr_stake_account) = self.get_account_with_fixed_root(&stake_pubkey) {
                    let pre_lamport = curr_stake_account.lamports();
                    let post_lamport = post_stake_account.lamports();
                    assert_eq!(
                        pre_lamport + u64::try_from(reward_amount).unwrap(),
                        post_lamport,
                        "stake account balance has changed since the reward calculation! \
                         account: {stake_pubkey}, pre balance: {pre_lamport}, \
                         post balance: {post_lamport}, rewards: {reward_amount}"
                    );
                }
            }
        }

        self.store_accounts((self.slot(), stake_rewards));
        stake_rewards
            .iter()
            .map(|stake_reward| stake_reward.stake_reward_info.lamports)
            .sum::<i64>() as u64
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::bank::{
            partitioned_epoch_rewards::epoch_rewards_hasher::hash_rewards_into_partitions,
            tests::create_genesis_config,
        },
        rand::Rng,
        solana_sdk::{
            account::from_account, epoch_schedule::EpochSchedule, feature_set, hash::Hash,
            native_token::LAMPORTS_PER_SOL, sysvar,
        },
    };

    #[test]
    fn test_distribute_partitioned_epoch_rewards() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let mut bank = Bank::new_for_tests(&genesis_config);

        let expected_num = 100;

        let stake_rewards = (0..expected_num)
            .map(|_| StakeReward::new_random())
            .collect::<Vec<_>>();

        let stake_rewards = hash_rewards_into_partitions(stake_rewards, &Hash::new(&[1; 32]), 2);

        bank.set_epoch_reward_status_active(stake_rewards);

        bank.distribute_partitioned_epoch_rewards();
    }

    #[test]
    #[should_panic(expected = "self.epoch_schedule.get_slots_in_epoch")]
    fn test_distribute_partitioned_epoch_rewards_too_many_partitions() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let mut bank = Bank::new_for_tests(&genesis_config);

        let expected_num = 1;

        let stake_rewards = (0..expected_num)
            .map(|_| StakeReward::new_random())
            .collect::<Vec<_>>();

        let stake_rewards = hash_rewards_into_partitions(
            stake_rewards,
            &Hash::new(&[1; 32]),
            bank.epoch_schedule().slots_per_epoch as usize + 1,
        );

        bank.set_epoch_reward_status_active(stake_rewards);

        bank.distribute_partitioned_epoch_rewards();
    }

    #[test]
    fn test_distribute_partitioned_epoch_rewards_empty() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let mut bank = Bank::new_for_tests(&genesis_config);

        bank.set_epoch_reward_status_active(vec![]);

        bank.distribute_partitioned_epoch_rewards();
    }

    /// Test distribute partitioned epoch rewards
    #[test]
    fn test_distribute_partitioned_epoch_rewards_bank_capital_and_sysvar_balance() {
        let (mut genesis_config, _mint_keypair) =
            create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        genesis_config.epoch_schedule = EpochSchedule::custom(432000, 432000, false);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.activate_feature(&feature_set::enable_partitioned_epoch_reward::id());

        // Set up epoch_rewards sysvar with rewards with 1e9 lamports to distribute.
        let total_rewards = 1_000_000_000;
        let num_partitions = 2; // num_partitions is arbitrary and unimportant for this test
        let total_points = (total_rewards * 42) as u128; // total_points is arbitrary for the purposes of this test
        bank.create_epoch_rewards_sysvar(total_rewards, 0, 42, num_partitions, total_points);
        let pre_epoch_rewards_account = bank.get_account(&sysvar::epoch_rewards::id()).unwrap();
        let expected_balance =
            bank.get_minimum_balance_for_rent_exemption(pre_epoch_rewards_account.data().len());
        // Expected balance is the sysvar rent-exempt balance
        assert_eq!(pre_epoch_rewards_account.lamports(), expected_balance);

        // Set up a partition of rewards to distribute
        let expected_num = 100;
        let mut stake_rewards = (0..expected_num)
            .map(|_| StakeReward::new_random())
            .collect::<Vec<_>>();
        let mut rewards_to_distribute = 0;
        for stake_reward in &mut stake_rewards {
            stake_reward.credit(100);
            rewards_to_distribute += 100;
        }
        let all_rewards = vec![stake_rewards];

        // Distribute rewards
        let pre_cap = bank.capitalization();
        bank.distribute_epoch_rewards_in_partition(&all_rewards, 0);
        let post_cap = bank.capitalization();
        let post_epoch_rewards_account = bank.get_account(&sysvar::epoch_rewards::id()).unwrap();

        // Assert that epoch rewards sysvar lamports balance does not change
        assert_eq!(post_epoch_rewards_account.lamports(), expected_balance);

        let epoch_rewards: sysvar::epoch_rewards::EpochRewards =
            from_account(&post_epoch_rewards_account).unwrap();
        assert_eq!(epoch_rewards.total_rewards, total_rewards);
        assert_eq!(epoch_rewards.distributed_rewards, rewards_to_distribute,);

        // Assert that the bank total capital changed by the amount of rewards
        // distributed
        assert_eq!(pre_cap + rewards_to_distribute, post_cap);
    }

    /// Test partitioned credits and reward history updates of epoch rewards do cover all the rewards
    /// slice.
    #[test]
    fn test_epoch_credit_rewards_and_history_update() {
        let (mut genesis_config, _mint_keypair) =
            create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        genesis_config.epoch_schedule = EpochSchedule::custom(432000, 432000, false);
        let mut bank = Bank::new_for_tests(&genesis_config);

        // setup the expected number of stake rewards
        let expected_num = 12345;

        let mut stake_rewards = (0..expected_num)
            .map(|_| StakeReward::new_random())
            .collect::<Vec<_>>();

        bank.store_accounts((bank.slot(), &stake_rewards[..]));

        // Simulate rewards
        let mut expected_rewards = 0;
        for stake_reward in &mut stake_rewards {
            stake_reward.credit(1);
            expected_rewards += 1;
        }

        let stake_rewards_bucket =
            hash_rewards_into_partitions(stake_rewards, &Hash::new(&[1; 32]), 100);
        bank.set_epoch_reward_status_active(stake_rewards_bucket.clone());

        // Test partitioned stores
        let mut total_rewards = 0;
        let mut total_num_updates = 0;

        let pre_update_history_len = bank.rewards.read().unwrap().len();

        for stake_rewards in stake_rewards_bucket {
            let total_rewards_in_lamports = bank.store_stake_accounts_in_partition(&stake_rewards);
            let num_history_updates = bank.update_reward_history_in_partition(&stake_rewards);
            assert_eq!(stake_rewards.len(), num_history_updates);
            total_rewards += total_rewards_in_lamports;
            total_num_updates += num_history_updates;
        }

        let post_update_history_len = bank.rewards.read().unwrap().len();

        // assert that all rewards are credited
        assert_eq!(total_rewards, expected_rewards);
        assert_eq!(total_num_updates, expected_num);
        assert_eq!(
            total_num_updates,
            post_update_history_len - pre_update_history_len
        );
    }

    #[test]
    fn test_update_reward_history_in_partition() {
        for zero_reward in [false, true] {
            let (genesis_config, _mint_keypair) =
                create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
            let bank = Bank::new_for_tests(&genesis_config);

            let mut expected_num = 100;

            let mut stake_rewards = (0..expected_num)
                .map(|_| StakeReward::new_random())
                .collect::<Vec<_>>();

            let mut rng = rand::thread_rng();
            let i_zero = rng.gen_range(0..expected_num);
            if zero_reward {
                // pick one entry to have zero rewards so it gets ignored
                stake_rewards[i_zero].stake_reward_info.lamports = 0;
            }

            let num_in_history = bank.update_reward_history_in_partition(&stake_rewards);

            if zero_reward {
                stake_rewards.remove(i_zero);
                // -1 because one of them had zero rewards and was ignored
                expected_num -= 1;
            }

            bank.rewards
                .read()
                .unwrap()
                .iter()
                .zip(stake_rewards.iter())
                .for_each(|((k, reward_info), expected_stake_reward)| {
                    assert_eq!(
                        (
                            &expected_stake_reward.stake_pubkey,
                            &expected_stake_reward.stake_reward_info
                        ),
                        (k, reward_info)
                    );
                });

            assert_eq!(num_in_history, expected_num);
        }
    }

    #[test]
    fn test_update_reward_history_in_partition_empty() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let bank = Bank::new_for_tests(&genesis_config);

        let stake_rewards = vec![];

        let num_in_history = bank.update_reward_history_in_partition(&stake_rewards);
        assert_eq!(num_in_history, 0);
    }

    /// Test rewards computation and partitioned rewards distribution at the epoch boundary
    #[test]
    fn test_store_stake_accounts_in_partition() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let bank = Bank::new_for_tests(&genesis_config);

        let expected_num = 100;

        let stake_rewards = (0..expected_num)
            .map(|_| StakeReward::new_random())
            .collect::<Vec<_>>();

        let expected_total = stake_rewards
            .iter()
            .map(|stake_reward| stake_reward.stake_reward_info.lamports)
            .sum::<i64>() as u64;

        let total_rewards_in_lamports = bank.store_stake_accounts_in_partition(&stake_rewards);
        assert_eq!(expected_total, total_rewards_in_lamports);
    }

    #[test]
    fn test_store_stake_accounts_in_partition_empty() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let bank = Bank::new_for_tests(&genesis_config);

        let stake_rewards = vec![];

        let expected_total = 0;

        let total_rewards_in_lamports = bank.store_stake_accounts_in_partition(&stake_rewards);
        assert_eq!(expected_total, total_rewards_in_lamports);
    }
}
