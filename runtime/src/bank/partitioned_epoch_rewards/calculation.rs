use {
    super::{
        epoch_rewards_hasher::hash_rewards_into_partitions, Bank,
        CalculateRewardsAndDistributeVoteRewardsResult, CalculateValidatorRewardsResult,
        EpochRewardCalculateParamInfo, PartitionedRewardsCalculation, PartitionedStakeReward,
        PartitionedStakeRewards, StakeRewardCalculation, StakeRewardCalculationPartitioned,
        VoteRewardsAccounts, REWARD_CALCULATION_NUM_BLOCKS,
    },
    crate::{
        bank::{
            PrevEpochInflationRewards, RewardCalcTracer, RewardCalculationEvent, RewardsMetrics,
            VoteAccount, VoteReward, VoteRewards,
        },
        stake_account::StakeAccount,
        stakes::Stakes,
    },
    dashmap::DashMap,
    log::{debug, info},
    rayon::{
        iter::{IntoParallelRefIterator, ParallelIterator},
        ThreadPool,
    },
    solana_measure::measure_us,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        account_utils::StateMut,
        clock::{Epoch, Slot},
        pubkey::Pubkey,
        reward_info::RewardInfo,
        reward_type::RewardType,
        stake::state::{Delegation, StakeStateV2},
        sysvar::epoch_rewards::EpochRewards,
    },
    solana_stake_program::points::PointValue,
    std::sync::atomic::{AtomicU64, Ordering::Relaxed},
};

impl Bank {
    /// Begin the process of calculating and distributing rewards.
    /// This process can take multiple slots.
    pub(in crate::bank) fn begin_partitioned_rewards(
        &mut self,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
        thread_pool: &ThreadPool,
        parent_epoch: Epoch,
        parent_slot: Slot,
        parent_block_height: u64,
        rewards_metrics: &mut RewardsMetrics,
    ) {
        let CalculateRewardsAndDistributeVoteRewardsResult {
            total_rewards,
            distributed_rewards,
            total_points,
            stake_rewards_by_partition,
        } = self.calculate_rewards_and_distribute_vote_rewards(
            parent_epoch,
            reward_calc_tracer,
            thread_pool,
            rewards_metrics,
        );

        let slot = self.slot();
        let distribution_starting_block_height =
            // For live-cluster testing pre-activation
            if self.force_partition_rewards_in_first_block_of_epoch() {
                self.block_height()
            } else {
                self.block_height() + REWARD_CALCULATION_NUM_BLOCKS
            };

        let num_partitions = stake_rewards_by_partition.len() as u64;

        self.set_epoch_reward_status_active(
            distribution_starting_block_height,
            stake_rewards_by_partition,
        );

        self.create_epoch_rewards_sysvar(
            total_rewards,
            distributed_rewards,
            distribution_starting_block_height,
            num_partitions,
            total_points,
        );

        datapoint_info!(
            "epoch-rewards-status-update",
            ("start_slot", slot, i64),
            ("calculation_block_height", self.block_height(), i64),
            ("active", 1, i64),
            ("parent_slot", parent_slot, i64),
            ("parent_block_height", parent_block_height, i64),
        );
    }

    // Calculate rewards from previous epoch and distribute vote rewards
    fn calculate_rewards_and_distribute_vote_rewards(
        &self,
        prev_epoch: Epoch,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
        thread_pool: &ThreadPool,
        metrics: &mut RewardsMetrics,
    ) -> CalculateRewardsAndDistributeVoteRewardsResult {
        let PartitionedRewardsCalculation {
            vote_account_rewards,
            stake_rewards_by_partition,
            old_vote_balance_and_staked,
            validator_rewards,
            validator_rate,
            foundation_rate,
            prev_epoch_duration_in_years,
            capitalization,
            total_points,
        } = self.calculate_rewards_for_partitioning(
            prev_epoch,
            reward_calc_tracer,
            thread_pool,
            metrics,
        );
        let vote_rewards = self.store_vote_accounts_partitioned(vote_account_rewards, metrics);

        // update reward history of JUST vote_rewards, stake_rewards is vec![] here
        self.update_reward_history(vec![], vote_rewards);

        let StakeRewardCalculationPartitioned {
            stake_rewards_by_partition,
            total_stake_rewards_lamports,
        } = stake_rewards_by_partition;

        // the remaining code mirrors `update_rewards_with_thread_pool()`

        let new_vote_balance_and_staked = self.stakes_cache.stakes().vote_balance_and_staked();

        // This is for vote rewards only.
        let validator_rewards_paid = new_vote_balance_and_staked - old_vote_balance_and_staked;
        self.assert_validator_rewards_paid(validator_rewards_paid);

        // verify that we didn't pay any more than we expected to
        assert!(validator_rewards >= validator_rewards_paid + total_stake_rewards_lamports);

        info!(
            "distributed vote rewards: {} out of {}, remaining {}",
            validator_rewards_paid, validator_rewards, total_stake_rewards_lamports
        );

        let (num_stake_accounts, num_vote_accounts) = {
            let stakes = self.stakes_cache.stakes();
            (
                stakes.stake_delegations().len(),
                stakes.vote_accounts().len(),
            )
        };
        self.capitalization
            .fetch_add(validator_rewards_paid, Relaxed);

        let active_stake = if let Some(stake_history_entry) =
            self.stakes_cache.stakes().history().get(prev_epoch)
        {
            stake_history_entry.effective
        } else {
            0
        };

        datapoint_info!(
            "epoch_rewards",
            ("slot", self.slot, i64),
            ("epoch", prev_epoch, i64),
            ("validator_rate", validator_rate, f64),
            ("foundation_rate", foundation_rate, f64),
            ("epoch_duration_in_years", prev_epoch_duration_in_years, f64),
            ("validator_rewards", validator_rewards_paid, i64),
            ("active_stake", active_stake, i64),
            ("pre_capitalization", capitalization, i64),
            ("post_capitalization", self.capitalization(), i64),
            ("num_stake_accounts", num_stake_accounts, i64),
            ("num_vote_accounts", num_vote_accounts, i64),
        );

        CalculateRewardsAndDistributeVoteRewardsResult {
            total_rewards: validator_rewards_paid + total_stake_rewards_lamports,
            distributed_rewards: validator_rewards_paid,
            total_points,
            stake_rewards_by_partition,
        }
    }

    fn store_vote_accounts_partitioned(
        &self,
        vote_account_rewards: VoteRewardsAccounts,
        metrics: &RewardsMetrics,
    ) -> Vec<(Pubkey, RewardInfo)> {
        let (_, measure_us) = measure_us!({
            // reformat data to make it not sparse.
            // `StorableAccounts` does not efficiently handle sparse data.
            // Not all entries in `vote_account_rewards.accounts_to_store` have a Some(account) to store.
            let to_store = vote_account_rewards
                .accounts_to_store
                .iter()
                .filter_map(|account| account.as_ref())
                .enumerate()
                .map(|(i, account)| (&vote_account_rewards.rewards[i].0, account))
                .collect::<Vec<_>>();
            self.store_accounts((self.slot(), &to_store[..]));
        });

        metrics
            .store_vote_accounts_us
            .fetch_add(measure_us, Relaxed);

        vote_account_rewards.rewards
    }

    /// Calculate rewards from previous epoch to prepare for partitioned distribution.
    pub(super) fn calculate_rewards_for_partitioning(
        &self,
        prev_epoch: Epoch,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
        thread_pool: &ThreadPool,
        metrics: &mut RewardsMetrics,
    ) -> PartitionedRewardsCalculation {
        let capitalization = self.capitalization();
        let PrevEpochInflationRewards {
            validator_rewards,
            prev_epoch_duration_in_years,
            validator_rate,
            foundation_rate,
        } = self.calculate_previous_epoch_inflation_rewards(capitalization, prev_epoch);

        let old_vote_balance_and_staked = self.stakes_cache.stakes().vote_balance_and_staked();

        let CalculateValidatorRewardsResult {
            vote_rewards_accounts: vote_account_rewards,
            stake_reward_calculation: mut stake_rewards,
            total_points,
        } = self
            .calculate_validator_rewards(
                prev_epoch,
                validator_rewards,
                reward_calc_tracer,
                thread_pool,
                metrics,
            )
            .unwrap_or_default();

        let num_partitions = self.get_reward_distribution_num_blocks(&stake_rewards.stake_rewards);
        let parent_blockhash = self
            .parent()
            .expect("Partitioned rewards calculation must still have access to parent Bank.")
            .last_blockhash();
        let stake_rewards_by_partition = hash_rewards_into_partitions(
            std::mem::take(&mut stake_rewards.stake_rewards),
            &parent_blockhash,
            num_partitions as usize,
        );

        PartitionedRewardsCalculation {
            vote_account_rewards,
            stake_rewards_by_partition: StakeRewardCalculationPartitioned {
                stake_rewards_by_partition,
                total_stake_rewards_lamports: stake_rewards.total_stake_rewards_lamports,
            },
            old_vote_balance_and_staked,
            validator_rewards,
            validator_rate,
            foundation_rate,
            prev_epoch_duration_in_years,
            capitalization,
            total_points,
        }
    }

    /// Calculate epoch reward and return vote and stake rewards.
    fn calculate_validator_rewards(
        &self,
        rewarded_epoch: Epoch,
        rewards: u64,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        thread_pool: &ThreadPool,
        metrics: &mut RewardsMetrics,
    ) -> Option<CalculateValidatorRewardsResult> {
        let stakes = self.stakes_cache.stakes();
        let reward_calculate_param = self.get_epoch_reward_calculate_param_info(&stakes);

        self.calculate_reward_points_partitioned(
            &reward_calculate_param,
            rewards,
            thread_pool,
            metrics,
        )
        .map(|point_value| {
            let total_points = point_value.points;
            let (vote_rewards_accounts, stake_reward_calculation) = self
                .calculate_stake_vote_rewards(
                    &reward_calculate_param,
                    rewarded_epoch,
                    point_value,
                    thread_pool,
                    reward_calc_tracer,
                    metrics,
                );
            CalculateValidatorRewardsResult {
                vote_rewards_accounts,
                stake_reward_calculation,
                total_points,
            }
        })
    }

    /// calculate and return some reward calc info to avoid recalculation across functions
    fn get_epoch_reward_calculate_param_info<'a>(
        &'a self,
        stakes: &'a Stakes<StakeAccount<Delegation>>,
    ) -> EpochRewardCalculateParamInfo<'a> {
        // Use `stakes` for stake-related info
        let stake_history = stakes.history().clone();
        let stake_delegations = self.filter_stake_delegations(stakes);

        // Use `EpochStakes` for vote accounts
        let leader_schedule_epoch = self.epoch_schedule().get_leader_schedule_epoch(self.slot());
        let cached_vote_accounts = self.epoch_stakes(leader_schedule_epoch)
            .expect("calculation should always run after Bank::update_epoch_stakes(leader_schedule_epoch)")
            .stakes()
            .vote_accounts();

        EpochRewardCalculateParamInfo {
            stake_history,
            stake_delegations,
            cached_vote_accounts,
        }
    }

    /// Calculates epoch rewards for stake/vote accounts
    /// Returns vote rewards, stake rewards, and the sum of all stake rewards in lamports
    fn calculate_stake_vote_rewards(
        &self,
        reward_calculate_params: &EpochRewardCalculateParamInfo,
        rewarded_epoch: Epoch,
        point_value: PointValue,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        metrics: &mut RewardsMetrics,
    ) -> (VoteRewardsAccounts, StakeRewardCalculation) {
        let EpochRewardCalculateParamInfo {
            stake_history,
            stake_delegations,
            cached_vote_accounts,
        } = reward_calculate_params;

        let solana_vote_program: Pubkey = solana_vote_program::id();

        let get_vote_account = |vote_pubkey: &Pubkey| -> Option<VoteAccount> {
            if let Some(vote_account) = cached_vote_accounts.get(vote_pubkey) {
                return Some(vote_account.clone());
            }
            // If accounts-db contains a valid vote account, then it should
            // already have been cached in cached_vote_accounts; so the code
            // below is only for sanity checking, and can be removed once
            // the cache is deemed to be reliable.
            let account = self.get_account_with_fixed_root(vote_pubkey)?;
            VoteAccount::try_from(account).ok()
        };

        let new_warmup_cooldown_rate_epoch = self.new_warmup_cooldown_rate_epoch();
        let vote_account_rewards: VoteRewards = DashMap::new();
        let total_stake_rewards = AtomicU64::default();
        let (stake_rewards, measure_stake_rewards_us) = measure_us!(thread_pool.install(|| {
            stake_delegations
                .par_iter()
                .filter_map(|(stake_pubkey, stake_account)| {
                    // curry closure to add the contextual stake_pubkey
                    let reward_calc_tracer = reward_calc_tracer.as_ref().map(|outer| {
                        // inner
                        move |inner_event: &_| {
                            outer(&RewardCalculationEvent::Staking(stake_pubkey, inner_event))
                        }
                    });

                    let stake_pubkey = **stake_pubkey;
                    let stake_account = (*stake_account).to_owned();

                    let vote_pubkey = stake_account.delegation().voter_pubkey;
                    let (mut stake_account, stake_state) =
                        <(AccountSharedData, StakeStateV2)>::from(stake_account);
                    let vote_account = get_vote_account(&vote_pubkey)?;
                    if vote_account.owner() != &solana_vote_program {
                        return None;
                    }
                    let vote_state = vote_account.vote_state().cloned().ok()?;

                    let pre_lamport = stake_account.lamports();

                    let redeemed = solana_stake_program::rewards::redeem_rewards(
                        rewarded_epoch,
                        stake_state,
                        &mut stake_account,
                        &vote_state,
                        &point_value,
                        stake_history,
                        reward_calc_tracer.as_ref(),
                        new_warmup_cooldown_rate_epoch,
                    );

                    let post_lamport = stake_account.lamports();

                    if let Ok((stakers_reward, voters_reward)) = redeemed {
                        debug!(
                            "calculated reward: {} {} {} {}",
                            stake_pubkey, pre_lamport, post_lamport, stakers_reward
                        );

                        // track voter rewards
                        let mut voters_reward_entry = vote_account_rewards
                            .entry(vote_pubkey)
                            .or_insert(VoteReward {
                                vote_account: vote_account.into(),
                                commission: vote_state.commission,
                                vote_rewards: 0,
                                vote_needs_store: false,
                            });

                        voters_reward_entry.vote_needs_store = true;
                        voters_reward_entry.vote_rewards = voters_reward_entry
                            .vote_rewards
                            .saturating_add(voters_reward);

                        let post_balance = stake_account.lamports();
                        total_stake_rewards.fetch_add(stakers_reward, Relaxed);

                        // Safe to unwrap on the following lines because all
                        // stake_delegations are type StakeAccount<Delegation>,
                        // which will always only wrap a `StakeStateV2::Stake`
                        // variant.
                        let updated_stake_state: StakeStateV2 = stake_account.state().unwrap();
                        let stake = updated_stake_state.stake().unwrap();
                        return Some(PartitionedStakeReward {
                            stake_pubkey,
                            stake_reward_info: RewardInfo {
                                reward_type: RewardType::Staking,
                                lamports: i64::try_from(stakers_reward).unwrap(),
                                post_balance,
                                commission: Some(vote_state.commission),
                            },
                            stake,
                        });
                    } else {
                        debug!(
                            "solana_stake_program::rewards::redeem_rewards() failed for {}: {:?}",
                            stake_pubkey, redeemed
                        );
                    }
                    None
                })
                .collect()
        }));
        let (vote_rewards, measure_vote_rewards_us) =
            measure_us!(Self::calc_vote_accounts_to_store(vote_account_rewards));

        metrics.redeem_rewards_us += measure_stake_rewards_us + measure_vote_rewards_us;

        (
            vote_rewards,
            StakeRewardCalculation {
                stake_rewards,
                total_stake_rewards_lamports: total_stake_rewards.load(Relaxed),
            },
        )
    }

    /// Calculates epoch reward points from stake/vote accounts.
    /// Returns reward lamports and points for the epoch or none if points == 0.
    fn calculate_reward_points_partitioned(
        &self,
        reward_calculate_params: &EpochRewardCalculateParamInfo,
        rewards: u64,
        thread_pool: &ThreadPool,
        metrics: &RewardsMetrics,
    ) -> Option<PointValue> {
        let EpochRewardCalculateParamInfo {
            stake_history,
            stake_delegations,
            cached_vote_accounts,
        } = reward_calculate_params;

        let solana_vote_program: Pubkey = solana_vote_program::id();

        let get_vote_account = |vote_pubkey: &Pubkey| -> Option<VoteAccount> {
            if let Some(vote_account) = cached_vote_accounts.get(vote_pubkey) {
                return Some(vote_account.clone());
            }
            // If accounts-db contains a valid vote account, then it should
            // already have been cached in cached_vote_accounts; so the code
            // below is only for sanity checking, and can be removed once
            // the cache is deemed to be reliable.
            let account = self.get_account_with_fixed_root(vote_pubkey)?;
            VoteAccount::try_from(account).ok()
        };

        let new_warmup_cooldown_rate_epoch = self.new_warmup_cooldown_rate_epoch();
        let (points, measure_us) = measure_us!(thread_pool.install(|| {
            stake_delegations
                .par_iter()
                .map(|(_stake_pubkey, stake_account)| {
                    let vote_pubkey = stake_account.delegation().voter_pubkey;

                    let Some(vote_account) = get_vote_account(&vote_pubkey) else {
                        return 0;
                    };
                    if vote_account.owner() != &solana_vote_program {
                        return 0;
                    }
                    let Ok(vote_state) = vote_account.vote_state() else {
                        return 0;
                    };

                    solana_stake_program::points::calculate_points(
                        stake_account.stake_state(),
                        vote_state,
                        stake_history,
                        new_warmup_cooldown_rate_epoch,
                    )
                    .unwrap_or(0)
                })
                .sum::<u128>()
        }));
        metrics.calculate_points_us.fetch_add(measure_us, Relaxed);

        (points > 0).then_some(PointValue { rewards, points })
    }

    /// If rewards are active, recalculates partitioned stake rewards and stores
    /// a new Bank::epoch_reward_status. This method assumes that vote rewards
    /// have already been calculated and delivered, and *only* recalculates
    /// stake rewards
    pub(in crate::bank) fn recalculate_partitioned_rewards(
        &mut self,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        thread_pool: &ThreadPool,
    ) {
        let epoch_rewards_sysvar = self.get_epoch_rewards_sysvar();
        if epoch_rewards_sysvar.active {
            let stake_rewards_by_partition = self.recalculate_stake_rewards(
                &epoch_rewards_sysvar,
                reward_calc_tracer,
                thread_pool,
            );
            self.set_epoch_reward_status_active(
                epoch_rewards_sysvar.distribution_starting_block_height,
                stake_rewards_by_partition,
            );
        }
    }

    /// Returns a vector of partitioned stake rewards. StakeRewards are
    /// recalculated from an active EpochRewards sysvar, vote accounts from
    /// EpochStakes, and stake accounts from StakesCache.
    fn recalculate_stake_rewards(
        &self,
        epoch_rewards_sysvar: &EpochRewards,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        thread_pool: &ThreadPool,
    ) -> Vec<PartitionedStakeRewards> {
        assert!(epoch_rewards_sysvar.active);
        // If rewards are active, the rewarded epoch is always the immediately
        // preceding epoch.
        let rewarded_epoch = self.epoch().saturating_sub(1);

        let point_value = PointValue {
            rewards: epoch_rewards_sysvar.total_rewards,
            points: epoch_rewards_sysvar.total_points,
        };

        let stakes = self.stakes_cache.stakes();
        let reward_calculate_param = self.get_epoch_reward_calculate_param_info(&stakes);

        // On recalculation, only the `StakeRewardCalculation::stake_rewards`
        // field is relevant. It is assumed that vote-account rewards have
        // already been calculated and delivered, while
        // `StakeRewardCalculation::total_rewards` only reflects rewards that
        // have not yet been distributed.
        let (_, StakeRewardCalculation { stake_rewards, .. }) = self.calculate_stake_vote_rewards(
            &reward_calculate_param,
            rewarded_epoch,
            point_value,
            thread_pool,
            reward_calc_tracer,
            &mut RewardsMetrics::default(), // This is required, but not reporting anything at the moment
        );
        drop(stakes);
        hash_rewards_into_partitions(
            stake_rewards,
            &epoch_rewards_sysvar.parent_blockhash,
            epoch_rewards_sysvar.num_partitions as usize,
        )
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            bank::{
                null_tracer,
                partitioned_epoch_rewards::{
                    tests::{
                        create_default_reward_bank, create_reward_bank, RewardBank, SLOTS_PER_EPOCH,
                    },
                    EpochRewardStatus, StartBlockHeightAndRewards,
                },
                tests::create_genesis_config,
                VoteReward,
            },
            stake_account::StakeAccount,
            stakes::Stakes,
        },
        rayon::ThreadPoolBuilder,
        solana_sdk::{
            account::{accounts_equal, ReadableAccount, WritableAccount},
            native_token::{sol_to_lamports, LAMPORTS_PER_SOL},
            reward_type::RewardType,
            stake::state::Delegation,
        },
        std::sync::{Arc, RwLockReadGuard},
    };

    #[test]
    fn test_store_vote_accounts_partitioned() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let bank = Bank::new_for_tests(&genesis_config);

        let expected_vote_rewards_num = 100;

        let vote_rewards = (0..expected_vote_rewards_num)
            .map(|_| Some((Pubkey::new_unique(), VoteReward::new_random())))
            .collect::<Vec<_>>();

        let mut vote_rewards_account = VoteRewardsAccounts::default();
        vote_rewards.iter().for_each(|e| {
            if let Some(p) = &e {
                let info = RewardInfo {
                    reward_type: RewardType::Voting,
                    lamports: p.1.vote_rewards as i64,
                    post_balance: p.1.vote_rewards,
                    commission: Some(p.1.commission),
                };
                vote_rewards_account.rewards.push((p.0, info));
                vote_rewards_account
                    .accounts_to_store
                    .push(e.as_ref().map(|p| p.1.vote_account.clone()));
            }
        });

        let metrics = RewardsMetrics::default();

        let stored_vote_accounts =
            bank.store_vote_accounts_partitioned(vote_rewards_account, &metrics);
        assert_eq!(expected_vote_rewards_num, stored_vote_accounts.len());

        // load accounts to make sure they were stored correctly
        vote_rewards.iter().for_each(|e| {
            if let Some(p) = &e {
                let (k, account) = (p.0, p.1.vote_account.clone());
                let loaded_account = bank.load_slow_with_fixed_root(&bank.ancestors, &k).unwrap();
                assert!(accounts_equal(&loaded_account.0, &account));
            }
        });
    }

    #[test]
    fn test_store_vote_accounts_partitioned_empty() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let bank = Bank::new_for_tests(&genesis_config);

        let expected = 0;
        let vote_rewards = VoteRewardsAccounts::default();
        let metrics = RewardsMetrics::default();

        let stored_vote_accounts = bank.store_vote_accounts_partitioned(vote_rewards, &metrics);
        assert_eq!(expected, stored_vote_accounts.len());
    }

    #[test]
    /// Test rewards computation and partitioned rewards distribution at the epoch boundary
    fn test_rewards_computation() {
        solana_logger::setup();

        let expected_num_delegations = 100;
        let bank = create_default_reward_bank(expected_num_delegations, SLOTS_PER_EPOCH).bank;

        // Calculate rewards
        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let mut rewards_metrics = RewardsMetrics::default();
        let expected_rewards = 100_000_000_000;

        let calculated_rewards = bank.calculate_validator_rewards(
            1,
            expected_rewards,
            null_tracer(),
            &thread_pool,
            &mut rewards_metrics,
        );

        let vote_rewards = &calculated_rewards.as_ref().unwrap().vote_rewards_accounts;
        let stake_rewards = &calculated_rewards
            .as_ref()
            .unwrap()
            .stake_reward_calculation;

        let total_vote_rewards: u64 = vote_rewards
            .rewards
            .iter()
            .map(|reward| reward.1.lamports)
            .sum::<i64>() as u64;

        // assert that total rewards matches the sum of vote rewards and stake rewards
        assert_eq!(
            stake_rewards.total_stake_rewards_lamports + total_vote_rewards,
            expected_rewards
        );

        // assert that number of stake rewards matches
        assert_eq!(stake_rewards.stake_rewards.len(), expected_num_delegations);
    }

    #[test]
    fn test_rewards_point_calculation() {
        solana_logger::setup();

        let expected_num_delegations = 100;
        let RewardBank { bank, .. } =
            create_default_reward_bank(expected_num_delegations, SLOTS_PER_EPOCH);

        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let rewards_metrics = RewardsMetrics::default();
        let expected_rewards = 100_000_000_000;

        let stakes: RwLockReadGuard<Stakes<StakeAccount<Delegation>>> = bank.stakes_cache.stakes();
        let reward_calculate_param = bank.get_epoch_reward_calculate_param_info(&stakes);

        let point_value = bank.calculate_reward_points_partitioned(
            &reward_calculate_param,
            expected_rewards,
            &thread_pool,
            &rewards_metrics,
        );

        assert!(point_value.is_some());
        assert_eq!(point_value.as_ref().unwrap().rewards, expected_rewards);
        assert_eq!(point_value.as_ref().unwrap().points, 8400000000000);
    }

    #[test]
    fn test_rewards_point_calculation_empty() {
        solana_logger::setup();

        // bank with no rewards to distribute
        let (genesis_config, _mint_keypair) = create_genesis_config(sol_to_lamports(1.0));
        let bank = Bank::new_for_tests(&genesis_config);

        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let rewards_metrics: RewardsMetrics = RewardsMetrics::default();
        let expected_rewards = 100_000_000_000;
        let stakes: RwLockReadGuard<Stakes<StakeAccount<Delegation>>> = bank.stakes_cache.stakes();
        let reward_calculate_param = bank.get_epoch_reward_calculate_param_info(&stakes);

        let point_value = bank.calculate_reward_points_partitioned(
            &reward_calculate_param,
            expected_rewards,
            &thread_pool,
            &rewards_metrics,
        );

        assert!(point_value.is_none());
    }

    #[test]
    fn test_calculate_stake_vote_rewards() {
        solana_logger::setup();

        let expected_num_delegations = 1;
        let RewardBank {
            bank,
            voters,
            stakers,
        } = create_default_reward_bank(expected_num_delegations, SLOTS_PER_EPOCH);

        let vote_pubkey = voters.first().unwrap();
        let mut vote_account = bank
            .load_slow_with_fixed_root(&bank.ancestors, vote_pubkey)
            .unwrap()
            .0;

        let stake_pubkey = stakers.first().unwrap();
        let stake_account = bank
            .load_slow_with_fixed_root(&bank.ancestors, stake_pubkey)
            .unwrap()
            .0;

        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let mut rewards_metrics = RewardsMetrics::default();

        let point_value = PointValue {
            rewards: 100000, // lamports to split
            points: 1000,    // over these points
        };
        let tracer = |_event: &RewardCalculationEvent| {};
        let reward_calc_tracer = Some(tracer);
        let rewarded_epoch = bank.epoch();
        let stakes: RwLockReadGuard<Stakes<StakeAccount<Delegation>>> = bank.stakes_cache.stakes();
        let reward_calculate_param = bank.get_epoch_reward_calculate_param_info(&stakes);
        let (vote_rewards_accounts, stake_reward_calculation) = bank.calculate_stake_vote_rewards(
            &reward_calculate_param,
            rewarded_epoch,
            point_value,
            &thread_pool,
            reward_calc_tracer,
            &mut rewards_metrics,
        );

        assert_eq!(
            vote_rewards_accounts.rewards.len(),
            vote_rewards_accounts.accounts_to_store.len()
        );
        assert_eq!(vote_rewards_accounts.rewards.len(), 1);
        let rewards = &vote_rewards_accounts.rewards[0];
        let account = &vote_rewards_accounts.accounts_to_store[0];
        let vote_rewards = 0;
        let commision = 0;
        _ = vote_account.checked_add_lamports(vote_rewards);
        assert_eq!(
            account.as_ref().unwrap().lamports(),
            vote_account.lamports()
        );
        assert!(accounts_equal(account.as_ref().unwrap(), &vote_account));
        assert_eq!(
            rewards.1,
            RewardInfo {
                reward_type: RewardType::Voting,
                lamports: vote_rewards as i64,
                post_balance: vote_account.lamports(),
                commission: Some(commision),
            }
        );
        assert_eq!(&rewards.0, vote_pubkey);

        assert_eq!(stake_reward_calculation.stake_rewards.len(), 1);
        assert_eq!(
            &stake_reward_calculation.stake_rewards[0].stake_pubkey,
            stake_pubkey
        );

        let original_stake_lamport = stake_account.lamports();
        let rewards = stake_reward_calculation.stake_rewards[0]
            .stake_reward_info
            .lamports;
        let expected_reward_info = RewardInfo {
            reward_type: RewardType::Staking,
            lamports: rewards,
            post_balance: original_stake_lamport + rewards as u64,
            commission: Some(commision),
        };
        assert_eq!(
            stake_reward_calculation.stake_rewards[0].stake_reward_info,
            expected_reward_info,
        );
    }

    fn compare_stake_rewards(
        expected_stake_rewards: &[PartitionedStakeRewards],
        received_stake_rewards: &[PartitionedStakeRewards],
    ) {
        for (i, partition) in received_stake_rewards.iter().enumerate() {
            let expected_partition = &expected_stake_rewards[i];
            assert_eq!(partition.len(), expected_partition.len());
            for reward in partition {
                assert!(expected_partition.iter().any(|x| x == reward));
            }
        }
    }

    #[test]
    fn test_recalculate_stake_rewards() {
        let expected_num_delegations = 4;
        let num_rewards_per_block = 2;
        // Distribute 4 rewards over 2 blocks
        let RewardBank { bank, .. } = create_reward_bank(
            expected_num_delegations,
            num_rewards_per_block,
            SLOTS_PER_EPOCH,
        );
        let rewarded_epoch = bank.epoch();

        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let mut rewards_metrics = RewardsMetrics::default();
        let PartitionedRewardsCalculation {
            stake_rewards_by_partition:
                StakeRewardCalculationPartitioned {
                    stake_rewards_by_partition: expected_stake_rewards,
                    ..
                },
            ..
        } = bank.calculate_rewards_for_partitioning(
            rewarded_epoch,
            null_tracer(),
            &thread_pool,
            &mut rewards_metrics,
        );

        let epoch_rewards_sysvar = bank.get_epoch_rewards_sysvar();
        let recalculated_rewards =
            bank.recalculate_stake_rewards(&epoch_rewards_sysvar, null_tracer(), &thread_pool);
        assert_eq!(expected_stake_rewards.len(), recalculated_rewards.len());
        compare_stake_rewards(&expected_stake_rewards, &recalculated_rewards);

        // Advance to first distribution block, ie. child block of the epoch
        // boundary; slot is advanced 2 to demonstrate that distribution works
        // on block-height, not slot
        let new_slot = bank.slot() + 2;
        let bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), new_slot));

        let epoch_rewards_sysvar = bank.get_epoch_rewards_sysvar();
        let recalculated_rewards =
            bank.recalculate_stake_rewards(&epoch_rewards_sysvar, null_tracer(), &thread_pool);
        assert_eq!(expected_stake_rewards.len(), recalculated_rewards.len());
        // First partition has already been distributed, so recalculation
        // returns 0 rewards
        assert_eq!(recalculated_rewards[0].len(), 0);
        let starting_index = (bank.block_height() + 1
            - epoch_rewards_sysvar.distribution_starting_block_height)
            as usize;
        compare_stake_rewards(
            &expected_stake_rewards[starting_index..],
            &recalculated_rewards[starting_index..],
        );

        // Advance to last distribution slot
        let new_slot = bank.slot() + 1;
        let bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), new_slot));

        let epoch_rewards_sysvar = bank.get_epoch_rewards_sysvar();
        assert!(!epoch_rewards_sysvar.active);
        // Recalculation would panic, tested separately
    }

    #[test]
    #[should_panic]
    fn test_recalculate_stake_rewards_distribution_complete() {
        let expected_num_delegations = 2;
        let num_rewards_per_block = 2;
        // Distribute 2 rewards over 1 block
        let RewardBank { bank, .. } = create_reward_bank(
            expected_num_delegations,
            num_rewards_per_block,
            SLOTS_PER_EPOCH,
        );
        let rewarded_epoch = bank.epoch();

        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let mut rewards_metrics = RewardsMetrics::default();
        let PartitionedRewardsCalculation {
            stake_rewards_by_partition:
                StakeRewardCalculationPartitioned {
                    stake_rewards_by_partition: expected_stake_rewards,
                    ..
                },
            ..
        } = bank.calculate_rewards_for_partitioning(
            rewarded_epoch,
            null_tracer(),
            &thread_pool,
            &mut rewards_metrics,
        );

        let epoch_rewards_sysvar = bank.get_epoch_rewards_sysvar();
        let recalculated_rewards =
            bank.recalculate_stake_rewards(&epoch_rewards_sysvar, null_tracer(), &thread_pool);
        assert_eq!(expected_stake_rewards.len(), recalculated_rewards.len());
        compare_stake_rewards(&expected_stake_rewards, &recalculated_rewards);

        // Advance to first distribution slot
        let new_slot = bank.slot() + 1;
        let bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), new_slot));

        let epoch_rewards_sysvar = bank.get_epoch_rewards_sysvar();
        assert!(!epoch_rewards_sysvar.active);
        // Should panic
        let _recalculated_rewards =
            bank.recalculate_stake_rewards(&epoch_rewards_sysvar, null_tracer(), &thread_pool);
    }

    #[test]
    fn test_recalculate_partitioned_rewards() {
        let expected_num_delegations = 4;
        let num_rewards_per_block = 2;
        // Distribute 4 rewards over 2 blocks
        let RewardBank { bank, .. } = create_reward_bank(
            expected_num_delegations,
            num_rewards_per_block,
            SLOTS_PER_EPOCH - 1,
        );
        let rewarded_epoch = bank.epoch();

        // Advance to next epoch boundary to update EpochStakes Kludgy because
        // mutable Bank methods require the bank not be Arc-wrapped.
        let new_slot = bank.slot() + 1;
        let mut bank = Bank::new_from_parent(bank, &Pubkey::default(), new_slot);
        let expected_starting_block_height = bank.block_height() + 1;

        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let mut rewards_metrics = RewardsMetrics::default();
        let PartitionedRewardsCalculation {
            stake_rewards_by_partition:
                StakeRewardCalculationPartitioned {
                    stake_rewards_by_partition: expected_stake_rewards,
                    ..
                },
            ..
        } = bank.calculate_rewards_for_partitioning(
            rewarded_epoch,
            null_tracer(),
            &thread_pool,
            &mut rewards_metrics,
        );

        bank.recalculate_partitioned_rewards(null_tracer(), &thread_pool);
        let EpochRewardStatus::Active(StartBlockHeightAndRewards {
            distribution_starting_block_height,
            stake_rewards_by_partition: ref recalculated_rewards,
        }) = bank.epoch_reward_status
        else {
            panic!("{:?} not active", bank.epoch_reward_status);
        };
        assert_eq!(
            expected_starting_block_height,
            distribution_starting_block_height
        );
        assert_eq!(expected_stake_rewards.len(), recalculated_rewards.len());
        compare_stake_rewards(&expected_stake_rewards, recalculated_rewards);

        // Advance to first distribution slot
        let mut bank =
            Bank::new_from_parent(Arc::new(bank), &Pubkey::default(), SLOTS_PER_EPOCH + 1);

        bank.recalculate_partitioned_rewards(null_tracer(), &thread_pool);
        let EpochRewardStatus::Active(StartBlockHeightAndRewards {
            distribution_starting_block_height,
            stake_rewards_by_partition: ref recalculated_rewards,
        }) = bank.epoch_reward_status
        else {
            panic!("{:?} not active", bank.epoch_reward_status);
        };
        assert_eq!(
            expected_starting_block_height,
            distribution_starting_block_height
        );
        assert_eq!(expected_stake_rewards.len(), recalculated_rewards.len());
        // First partition has already been distributed, so recalculation
        // returns 0 rewards
        assert_eq!(recalculated_rewards[0].len(), 0);
        let epoch_rewards_sysvar = bank.get_epoch_rewards_sysvar();
        let starting_index = (bank.block_height() + 1
            - epoch_rewards_sysvar.distribution_starting_block_height)
            as usize;
        compare_stake_rewards(
            &expected_stake_rewards[starting_index..],
            &recalculated_rewards[starting_index..],
        );

        // Advance to last distribution slot
        let mut bank =
            Bank::new_from_parent(Arc::new(bank), &Pubkey::default(), SLOTS_PER_EPOCH + 2);

        bank.recalculate_partitioned_rewards(null_tracer(), &thread_pool);
        assert_eq!(bank.epoch_reward_status, EpochRewardStatus::Inactive);
    }
}
