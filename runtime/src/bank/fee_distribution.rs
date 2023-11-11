use {
    super::Bank,
    log::{debug, warn},
    solana_accounts_db::{account_rent_state::RentState, stake_rewards::RewardInfo},
    solana_sdk::{
        account::{ReadableAccount, WritableAccount},
        pubkey::Pubkey,
        reward_type::RewardType,
        system_program,
    },
    solana_vote::vote_account::VoteAccountsHashMap,
    std::{result::Result, sync::atomic::Ordering::Relaxed},
    thiserror::Error,
};

#[derive(Debug)]
struct DepositFeeOptions {
    check_account_owner: bool,
    check_rent_paying: bool,
}

#[derive(Error, Debug, PartialEq)]
enum DepositFeeError {
    #[error("fee account became rent paying")]
    InvalidRentPayingAccount,
    #[error("lamport overflow")]
    LamportOverflow,
    #[error("invalid fee account owner")]
    InvalidAccountOwner,
}

impl Bank {
    // Distribute collected transaction fees for this slot to collector_id (= current leader).
    //
    // Each validator is incentivized to process more transactions to earn more transaction fees.
    // Transaction fees are rewarded for the computing resource utilization cost, directly
    // proportional to their actual processing power.
    //
    // collector_id is rotated according to stake-weighted leader schedule. So the opportunity of
    // earning transaction fees are fairly distributed by stake. And missing the opportunity
    // (not producing a block as a leader) earns nothing. So, being online is incentivized as a
    // form of transaction fees as well.
    //
    // On the other hand, rent fees are distributed under slightly different philosophy, while
    // still being stake-weighted.
    // Ref: distribute_rent_to_validators
    pub(super) fn distribute_transaction_fees(&self) {
        let collector_fees = self.collector_fees.load(Relaxed);
        if collector_fees != 0 {
            let (deposit, mut burn) = self.fee_rate_governor.burn(collector_fees);
            if deposit > 0 {
                let validate_fee_collector = self.validate_fee_collector_account();
                match self.deposit_fees(
                    &self.collector_id,
                    deposit,
                    DepositFeeOptions {
                        check_account_owner: validate_fee_collector,
                        check_rent_paying: validate_fee_collector,
                    },
                ) {
                    Ok(post_balance) => {
                        self.rewards.write().unwrap().push((
                            self.collector_id,
                            RewardInfo {
                                reward_type: RewardType::Fee,
                                lamports: deposit as i64,
                                post_balance,
                                commission: None,
                            },
                        ));
                    }
                    Err(err) => {
                        debug!(
                            "Burned {} lamport tx fee instead of sending to {} due to {}",
                            deposit, self.collector_id, err
                        );
                        datapoint_warn!(
                            "bank-burned_fee",
                            ("slot", self.slot(), i64),
                            ("num_lamports", deposit, i64),
                            ("error", err.to_string(), String),
                        );
                        burn += deposit;
                    }
                }
            }
            self.capitalization.fetch_sub(burn, Relaxed);
        }
    }

    // Deposits fees into a specified account and if successful, returns the new balance of that account
    fn deposit_fees(
        &self,
        pubkey: &Pubkey,
        fees: u64,
        options: DepositFeeOptions,
    ) -> Result<u64, DepositFeeError> {
        let mut account = self.get_account_with_fixed_root(pubkey).unwrap_or_default();

        if options.check_account_owner && !system_program::check_id(account.owner()) {
            return Err(DepositFeeError::InvalidAccountOwner);
        }

        let rent = self.rent_collector().rent;
        let recipient_pre_rent_state = RentState::from_account(&account, &rent);
        let distribution = account.checked_add_lamports(fees);
        if distribution.is_err() {
            return Err(DepositFeeError::LamportOverflow);
        }
        if options.check_rent_paying {
            let recipient_post_rent_state = RentState::from_account(&account, &rent);
            let rent_state_transition_allowed =
                recipient_post_rent_state.transition_allowed_from(&recipient_pre_rent_state);
            if !rent_state_transition_allowed {
                return Err(DepositFeeError::InvalidRentPayingAccount);
            }
        }

        self.store_account(pubkey, &account);
        Ok(account.lamports())
    }

    // Distribute collected rent fees for this slot to staked validators (excluding stakers)
    // according to stake.
    //
    // The nature of rent fee is the cost of doing business, every validator has to hold (or have
    // access to) the same list of accounts, so we pay according to stake, which is a rough proxy for
    // value to the network.
    //
    // Currently, rent distribution doesn't consider given validator's uptime at all (this might
    // change). That's because rent should be rewarded for the storage resource utilization cost.
    // It's treated differently from transaction fees, which is for the computing resource
    // utilization cost.
    //
    // We can't use collector_id (which is rotated according to stake-weighted leader schedule)
    // as an approximation to the ideal rent distribution to simplify and avoid this per-slot
    // computation for the distribution (time: N log N, space: N acct. stores; N = # of
    // validators).
    // The reason is that rent fee doesn't need to be incentivized for throughput unlike transaction
    // fees
    //
    // Ref: distribute_transaction_fees
    #[allow(clippy::needless_collect)]
    fn distribute_rent_to_validators(
        &self,
        vote_accounts: &VoteAccountsHashMap,
        rent_to_be_distributed: u64,
    ) {
        let mut total_staked = 0;

        // Collect the stake associated with each validator.
        // Note that a validator may be present in this vector multiple times if it happens to have
        // more than one staked vote account somehow
        let mut validator_stakes = vote_accounts
            .iter()
            .filter_map(|(_vote_pubkey, (staked, account))| {
                if *staked == 0 {
                    None
                } else {
                    total_staked += *staked;
                    Some((account.node_pubkey()?, *staked))
                }
            })
            .collect::<Vec<(Pubkey, u64)>>();

        #[cfg(test)]
        if validator_stakes.is_empty() {
            // some tests bank.freezes() with bad staking state
            self.capitalization
                .fetch_sub(rent_to_be_distributed, Relaxed);
            return;
        }
        #[cfg(not(test))]
        assert!(!validator_stakes.is_empty());

        // Sort first by stake and then by validator identity pubkey for determinism.
        // If two items are still equal, their relative order does not matter since
        // both refer to the same validator.
        validator_stakes.sort_unstable_by(|(pubkey1, staked1), (pubkey2, staked2)| {
            (staked1, pubkey1).cmp(&(staked2, pubkey2)).reverse()
        });

        let enforce_fix = self.no_overflow_rent_distribution_enabled();

        let mut rent_distributed_in_initial_round = 0;
        let validator_rent_shares = validator_stakes
            .into_iter()
            .map(|(pubkey, staked)| {
                let rent_share = if !enforce_fix {
                    (((staked * rent_to_be_distributed) as f64) / (total_staked as f64)) as u64
                } else {
                    (((staked as u128) * (rent_to_be_distributed as u128)) / (total_staked as u128))
                        .try_into()
                        .unwrap()
                };
                rent_distributed_in_initial_round += rent_share;
                (pubkey, rent_share)
            })
            .collect::<Vec<(Pubkey, u64)>>();

        // Leftover lamports after fraction calculation, will be paid to validators starting from highest stake
        // holder
        let mut leftover_lamports = rent_to_be_distributed - rent_distributed_in_initial_round;

        let mut rent_to_burn: u64 = 0;
        let mut rewards = vec![];
        validator_rent_shares
            .into_iter()
            .for_each(|(pubkey, rent_share)| {
                let rent_to_be_paid = if leftover_lamports > 0 {
                    leftover_lamports -= 1;
                    rent_share + 1
                } else {
                    rent_share
                };
                if !enforce_fix || rent_to_be_paid > 0 {
                    let check_account_owner = self.validate_fee_collector_account();
                    let check_rent_paying = self.prevent_rent_paying_rent_recipients();
                    match self.deposit_fees(
                        &pubkey,
                        rent_to_be_paid,
                        DepositFeeOptions {
                            check_account_owner,
                            check_rent_paying,
                        },
                    ) {
                        Ok(post_balance) => {
                            rewards.push((
                                pubkey,
                                RewardInfo {
                                    reward_type: RewardType::Rent,
                                    lamports: rent_to_be_paid as i64,
                                    post_balance,
                                    commission: None,
                                },
                            ));
                        }
                        Err(err) => {
                            debug!(
                                "Burned {} lamport rent fee instead of sending to {} due to {}",
                                rent_to_be_paid, pubkey, err
                            );

                            // overflow adding lamports or resulting account is invalid
                            // so burn lamports and track lamports burned per slot
                            rent_to_burn = rent_to_burn.saturating_add(rent_to_be_paid);
                        }
                    }
                }
            });
        self.rewards.write().unwrap().append(&mut rewards);

        if rent_to_burn > 0 {
            self.capitalization.fetch_sub(rent_to_burn, Relaxed);
            datapoint_warn!(
                "bank-burned_rent",
                ("slot", self.slot(), i64),
                ("num_lamports", rent_to_burn, i64)
            );
        }

        if enforce_fix {
            assert_eq!(leftover_lamports, 0);
        } else if leftover_lamports != 0 {
            warn!(
                "There was leftover from rent distribution: {}",
                leftover_lamports
            );
            self.capitalization.fetch_sub(leftover_lamports, Relaxed);
        }
    }

    pub(super) fn distribute_rent_fees(&self) {
        let total_rent_collected = self.collected_rent.load(Relaxed);

        let (burned_portion, rent_to_be_distributed) = self
            .rent_collector
            .rent
            .calculate_burn(total_rent_collected);

        debug!(
            "distributed rent: {} (rounded from: {}, burned: {})",
            rent_to_be_distributed, total_rent_collected, burned_portion
        );
        self.capitalization.fetch_sub(burned_portion, Relaxed);

        if rent_to_be_distributed == 0 {
            return;
        }

        self.distribute_rent_to_validators(&self.vote_accounts(), rent_to_be_distributed);
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::genesis_utils::{
            create_genesis_config, create_genesis_config_with_leader,
            create_genesis_config_with_vote_accounts, ValidatorVoteKeypairs,
        },
        log::info,
        solana_sdk::{
            account::AccountSharedData, feature_set, native_token::sol_to_lamports, pubkey,
            rent::Rent, signature::Signer,
        },
    };

    #[test]
    fn test_distribute_transaction_fees() {
        #[derive(PartialEq)]
        enum Scenario {
            Normal,
            InvalidOwner,
            RentPaying,
        }

        struct TestCase {
            scenario: Scenario,
            disable_checks: bool,
        }

        impl TestCase {
            fn new(scenario: Scenario, disable_checks: bool) -> Self {
                Self {
                    scenario,
                    disable_checks,
                }
            }
        }

        for test_case in [
            TestCase::new(Scenario::Normal, false),
            TestCase::new(Scenario::Normal, true),
            TestCase::new(Scenario::InvalidOwner, false),
            TestCase::new(Scenario::InvalidOwner, true),
            TestCase::new(Scenario::RentPaying, false),
            TestCase::new(Scenario::RentPaying, true),
        ] {
            let mut genesis = create_genesis_config(0);
            if test_case.disable_checks {
                genesis
                    .genesis_config
                    .accounts
                    .remove(&feature_set::validate_fee_collector_account::id())
                    .unwrap();
            }
            let rent = Rent::default();
            let min_rent_exempt_balance = rent.minimum_balance(0);
            genesis.genesis_config.rent = rent; // Ensure rent is non-zero, as genesis_utils sets Rent::free by default
            let bank = Bank::new_for_tests(&genesis.genesis_config);
            let transaction_fees = 100;
            bank.collector_fees.fetch_add(transaction_fees, Relaxed);
            assert_eq!(transaction_fees, bank.collector_fees.load(Relaxed));
            let (expected_collected_fees, burn_amount) =
                bank.fee_rate_governor.burn(transaction_fees);
            assert!(burn_amount > 0);

            if test_case.scenario == Scenario::RentPaying {
                // ensure that account balance + collected fees will make it rent-paying
                let initial_balance = 100;
                let account = AccountSharedData::new(initial_balance, 0, &system_program::id());
                bank.store_account(bank.collector_id(), &account);
                assert!(initial_balance + transaction_fees < min_rent_exempt_balance);
            } else if test_case.scenario == Scenario::InvalidOwner {
                // ensure that account owner is invalid and fee distribution will fail
                let account =
                    AccountSharedData::new(min_rent_exempt_balance, 0, &Pubkey::new_unique());
                bank.store_account(bank.collector_id(), &account);
            } else {
                let account =
                    AccountSharedData::new(min_rent_exempt_balance, 0, &system_program::id());
                bank.store_account(bank.collector_id(), &account);
            }

            let initial_capitalization = bank.capitalization();
            let initial_collector_id_balance = bank.get_balance(bank.collector_id());
            bank.distribute_transaction_fees();
            let new_collector_id_balance = bank.get_balance(bank.collector_id());

            if test_case.scenario != Scenario::Normal && !test_case.disable_checks {
                assert_eq!(initial_collector_id_balance, new_collector_id_balance);
                assert_eq!(
                    initial_capitalization - transaction_fees,
                    bank.capitalization()
                );
                let locked_rewards = bank.rewards.read().unwrap();
                assert!(
                    locked_rewards.is_empty(),
                    "There should be no rewards distributed"
                );
            } else {
                assert_eq!(
                    initial_collector_id_balance + expected_collected_fees,
                    new_collector_id_balance
                );

                assert_eq!(initial_capitalization - burn_amount, bank.capitalization());

                let locked_rewards = bank.rewards.read().unwrap();
                assert_eq!(
                    locked_rewards.len(),
                    1,
                    "There should be one reward distributed"
                );

                let reward_info = &locked_rewards[0];
                assert_eq!(
                    reward_info.1.lamports, expected_collected_fees as i64,
                    "The reward amount should match the expected deposit"
                );
                assert_eq!(
                    reward_info.1.reward_type,
                    RewardType::Fee,
                    "The reward type should be Fee"
                );
            }
        }
    }

    #[test]
    fn test_distribute_transaction_fees_zero() {
        let genesis = create_genesis_config(0);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        assert_eq!(bank.collector_fees.load(Relaxed), 0);

        let initial_capitalization = bank.capitalization();
        let initial_collector_id_balance = bank.get_balance(bank.collector_id());
        bank.distribute_transaction_fees();
        let new_collector_id_balance = bank.get_balance(bank.collector_id());

        assert_eq!(initial_collector_id_balance, new_collector_id_balance);
        assert_eq!(initial_capitalization, bank.capitalization());
        let locked_rewards = bank.rewards.read().unwrap();
        assert!(
            locked_rewards.is_empty(),
            "There should be no rewards distributed"
        );
    }

    #[test]
    fn test_distribute_transaction_fees_burn_all() {
        let mut genesis = create_genesis_config(0);
        genesis.genesis_config.fee_rate_governor.burn_percent = 100;
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let transaction_fees = 100;
        bank.collector_fees.fetch_add(transaction_fees, Relaxed);
        assert_eq!(transaction_fees, bank.collector_fees.load(Relaxed));

        let initial_capitalization = bank.capitalization();
        let initial_collector_id_balance = bank.get_balance(bank.collector_id());
        bank.distribute_transaction_fees();
        let new_collector_id_balance = bank.get_balance(bank.collector_id());

        assert_eq!(initial_collector_id_balance, new_collector_id_balance);
        assert_eq!(
            initial_capitalization - transaction_fees,
            bank.capitalization()
        );
        let locked_rewards = bank.rewards.read().unwrap();
        assert!(
            locked_rewards.is_empty(),
            "There should be no rewards distributed"
        );
    }

    #[test]
    fn test_distribute_transaction_fees_overflow_failure() {
        let genesis = create_genesis_config(0);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let transaction_fees = 100;
        bank.collector_fees.fetch_add(transaction_fees, Relaxed);
        assert_eq!(transaction_fees, bank.collector_fees.load(Relaxed));

        // ensure that account balance will overflow and fee distribution will fail
        let account = AccountSharedData::new(u64::MAX, 0, &system_program::id());
        bank.store_account(bank.collector_id(), &account);

        let initial_capitalization = bank.capitalization();
        let initial_collector_id_balance = bank.get_balance(bank.collector_id());
        bank.distribute_transaction_fees();
        let new_collector_id_balance = bank.get_balance(bank.collector_id());

        assert_eq!(initial_collector_id_balance, new_collector_id_balance);
        assert_eq!(
            initial_capitalization - transaction_fees,
            bank.capitalization()
        );
        let locked_rewards = bank.rewards.read().unwrap();
        assert!(
            locked_rewards.is_empty(),
            "There should be no rewards distributed"
        );
    }

    #[test]
    fn test_deposit_fees() {
        let initial_balance = 1_000_000_000;
        let genesis = create_genesis_config(initial_balance);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let pubkey = genesis.mint_keypair.pubkey();

        let deposit_amount = 500;
        let options = DepositFeeOptions {
            check_account_owner: true,
            check_rent_paying: true,
        };

        assert_eq!(
            bank.deposit_fees(&pubkey, deposit_amount, options),
            Ok(initial_balance + deposit_amount),
            "New balance should be the sum of the initial balance and deposit amount"
        );
    }

    #[test]
    fn test_deposit_fees_with_overflow() {
        let initial_balance = u64::MAX;
        let genesis = create_genesis_config(initial_balance);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let pubkey = genesis.mint_keypair.pubkey();

        let deposit_amount = 500;
        let options = DepositFeeOptions {
            check_account_owner: false,
            check_rent_paying: false,
        };

        assert_eq!(
            bank.deposit_fees(&pubkey, deposit_amount, options),
            Err(DepositFeeError::LamportOverflow),
            "Expected an error due to lamport overflow"
        );
    }

    #[test]
    fn test_deposit_fees_invalid_account_owner() {
        let initial_balance = 1000;
        let genesis = create_genesis_config_with_leader(0, &pubkey::new_rand(), initial_balance);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let pubkey = genesis.voting_keypair.pubkey();

        let deposit_amount = 500;

        // enable check_account_owner
        {
            let options = DepositFeeOptions {
                check_account_owner: true, // Intentionally checking for account owner
                check_rent_paying: false,
            };

            assert_eq!(
                bank.deposit_fees(&pubkey, deposit_amount, options),
                Err(DepositFeeError::InvalidAccountOwner),
                "Expected an error due to invalid account owner"
            );
        }

        // disable check_account_owner
        {
            let options = DepositFeeOptions {
                check_account_owner: false,
                check_rent_paying: false,
            };

            assert_eq!(
                bank.deposit_fees(&pubkey, deposit_amount, options),
                Ok(initial_balance + deposit_amount),
                "New balance should be the sum of the initial balance and deposit amount"
            );
        }
    }

    #[test]
    fn test_deposit_fees_invalid_rent_paying() {
        let initial_balance = 0;
        let genesis = create_genesis_config(initial_balance);
        let pubkey = genesis.mint_keypair.pubkey();
        let mut genesis_config = genesis.genesis_config;
        let rent = Rent::default();
        genesis_config.rent = rent; // Ensure rent is non-zero, as genesis_utils sets Rent::free by default
        let bank = Bank::new_for_tests(&genesis_config);
        let min_rent_exempt_balance = rent.minimum_balance(0);

        let deposit_amount = 500;
        assert!(initial_balance + deposit_amount < min_rent_exempt_balance);

        // enable check_rent_paying
        {
            let options = DepositFeeOptions {
                check_account_owner: false,
                check_rent_paying: true,
            };

            assert_eq!(
                bank.deposit_fees(&pubkey, deposit_amount, options),
                Err(DepositFeeError::InvalidRentPayingAccount),
                "Expected an error due to invalid rent paying account"
            );
        }

        // disable check_rent_paying
        {
            let options = DepositFeeOptions {
                check_account_owner: false,
                check_rent_paying: false,
            };

            assert_eq!(
                bank.deposit_fees(&pubkey, deposit_amount, options),
                Ok(initial_balance + deposit_amount),
                "New balance should be the sum of the initial balance and deposit amount"
            );
        }
    }

    #[test]
    fn test_distribute_rent_to_validators_overflow() {
        solana_logger::setup();

        // These values are taken from the real cluster (testnet)
        const RENT_TO_BE_DISTRIBUTED: u64 = 120_525;
        const VALIDATOR_STAKE: u64 = 374_999_998_287_840;

        let validator_pubkey = solana_sdk::pubkey::new_rand();
        let mut genesis_config =
            create_genesis_config_with_leader(10, &validator_pubkey, VALIDATOR_STAKE)
                .genesis_config;

        let bank = Bank::new_for_tests(&genesis_config);
        let old_validator_lamports = bank.get_balance(&validator_pubkey);
        bank.distribute_rent_to_validators(&bank.vote_accounts(), RENT_TO_BE_DISTRIBUTED);
        let new_validator_lamports = bank.get_balance(&validator_pubkey);
        assert_eq!(
            new_validator_lamports,
            old_validator_lamports + RENT_TO_BE_DISTRIBUTED
        );

        genesis_config
            .accounts
            .remove(&feature_set::no_overflow_rent_distribution::id())
            .unwrap();
        let bank = std::panic::AssertUnwindSafe(Bank::new_for_tests(&genesis_config));
        let old_validator_lamports = bank.get_balance(&validator_pubkey);
        let new_validator_lamports = std::panic::catch_unwind(|| {
            bank.distribute_rent_to_validators(&bank.vote_accounts(), RENT_TO_BE_DISTRIBUTED);
            bank.get_balance(&validator_pubkey)
        });

        if let Ok(new_validator_lamports) = new_validator_lamports {
            info!("asserting overflowing incorrect rent distribution");
            assert_ne!(
                new_validator_lamports,
                old_validator_lamports + RENT_TO_BE_DISTRIBUTED
            );
        } else {
            info!("NOT-asserting overflowing incorrect rent distribution");
        }
    }

    #[test]
    fn test_distribute_rent_to_validators_rent_paying() {
        solana_logger::setup();

        const RENT_PER_VALIDATOR: u64 = 55;
        const TOTAL_RENT: u64 = RENT_PER_VALIDATOR * 4;

        let empty_validator = ValidatorVoteKeypairs::new_rand();
        let rent_paying_validator = ValidatorVoteKeypairs::new_rand();
        let becomes_rent_exempt_validator = ValidatorVoteKeypairs::new_rand();
        let rent_exempt_validator = ValidatorVoteKeypairs::new_rand();
        let keypairs = vec![
            &empty_validator,
            &rent_paying_validator,
            &becomes_rent_exempt_validator,
            &rent_exempt_validator,
        ];
        let genesis_config_info = create_genesis_config_with_vote_accounts(
            sol_to_lamports(1000.),
            &keypairs,
            vec![sol_to_lamports(1000.); 4],
        );
        let mut genesis_config = genesis_config_info.genesis_config;
        genesis_config.rent = Rent::default(); // Ensure rent is non-zero, as genesis_utils sets Rent::free by default

        for deactivate_feature in [false, true] {
            if deactivate_feature {
                genesis_config
                    .accounts
                    .remove(&feature_set::prevent_rent_paying_rent_recipients::id())
                    .unwrap();
            }
            let bank = Bank::new_for_tests(&genesis_config);
            let rent = bank.rent_collector().rent;
            let rent_exempt_minimum = rent.minimum_balance(0);

            // Make one validator have an empty identity account
            let mut empty_validator_account = bank
                .get_account_with_fixed_root(&empty_validator.node_keypair.pubkey())
                .unwrap();
            empty_validator_account.set_lamports(0);
            bank.store_account(
                &empty_validator.node_keypair.pubkey(),
                &empty_validator_account,
            );

            // Make one validator almost rent-exempt, less RENT_PER_VALIDATOR
            let mut becomes_rent_exempt_validator_account = bank
                .get_account_with_fixed_root(&becomes_rent_exempt_validator.node_keypair.pubkey())
                .unwrap();
            becomes_rent_exempt_validator_account
                .set_lamports(rent_exempt_minimum - RENT_PER_VALIDATOR);
            bank.store_account(
                &becomes_rent_exempt_validator.node_keypair.pubkey(),
                &becomes_rent_exempt_validator_account,
            );

            // Make one validator rent-exempt
            let mut rent_exempt_validator_account = bank
                .get_account_with_fixed_root(&rent_exempt_validator.node_keypair.pubkey())
                .unwrap();
            rent_exempt_validator_account.set_lamports(rent_exempt_minimum);
            bank.store_account(
                &rent_exempt_validator.node_keypair.pubkey(),
                &rent_exempt_validator_account,
            );

            let get_rent_state = |bank: &Bank, address: &Pubkey| -> RentState {
                let account = bank
                    .get_account_with_fixed_root(address)
                    .unwrap_or_default();
                RentState::from_account(&account, &rent)
            };

            // Assert starting RentStates
            assert_eq!(
                get_rent_state(&bank, &empty_validator.node_keypair.pubkey()),
                RentState::Uninitialized
            );
            assert_eq!(
                get_rent_state(&bank, &rent_paying_validator.node_keypair.pubkey()),
                RentState::RentPaying {
                    lamports: 42,
                    data_size: 0,
                }
            );
            assert_eq!(
                get_rent_state(&bank, &becomes_rent_exempt_validator.node_keypair.pubkey()),
                RentState::RentPaying {
                    lamports: rent_exempt_minimum - RENT_PER_VALIDATOR,
                    data_size: 0,
                }
            );
            assert_eq!(
                get_rent_state(&bank, &rent_exempt_validator.node_keypair.pubkey()),
                RentState::RentExempt
            );

            let old_empty_validator_lamports =
                bank.get_balance(&empty_validator.node_keypair.pubkey());
            let old_rent_paying_validator_lamports =
                bank.get_balance(&rent_paying_validator.node_keypair.pubkey());
            let old_becomes_rent_exempt_validator_lamports =
                bank.get_balance(&becomes_rent_exempt_validator.node_keypair.pubkey());
            let old_rent_exempt_validator_lamports =
                bank.get_balance(&rent_exempt_validator.node_keypair.pubkey());

            bank.distribute_rent_to_validators(&bank.vote_accounts(), TOTAL_RENT);

            let new_empty_validator_lamports =
                bank.get_balance(&empty_validator.node_keypair.pubkey());
            let new_rent_paying_validator_lamports =
                bank.get_balance(&rent_paying_validator.node_keypair.pubkey());
            let new_becomes_rent_exempt_validator_lamports =
                bank.get_balance(&becomes_rent_exempt_validator.node_keypair.pubkey());
            let new_rent_exempt_validator_lamports =
                bank.get_balance(&rent_exempt_validator.node_keypair.pubkey());

            // Assert ending balances; rent should be withheld if test is active and ending RentState
            // is RentPaying, ie. empty_validator and rent_paying_validator
            assert_eq!(
                if deactivate_feature {
                    old_empty_validator_lamports + RENT_PER_VALIDATOR
                } else {
                    old_empty_validator_lamports
                },
                new_empty_validator_lamports
            );

            assert_eq!(
                if deactivate_feature {
                    old_rent_paying_validator_lamports + RENT_PER_VALIDATOR
                } else {
                    old_rent_paying_validator_lamports
                },
                new_rent_paying_validator_lamports
            );

            assert_eq!(
                old_becomes_rent_exempt_validator_lamports + RENT_PER_VALIDATOR,
                new_becomes_rent_exempt_validator_lamports
            );

            assert_eq!(
                old_rent_exempt_validator_lamports + RENT_PER_VALIDATOR,
                new_rent_exempt_validator_lamports
            );

            // Assert ending RentStates
            assert_eq!(
                if deactivate_feature {
                    RentState::RentPaying {
                        lamports: RENT_PER_VALIDATOR,
                        data_size: 0,
                    }
                } else {
                    RentState::Uninitialized
                },
                get_rent_state(&bank, &empty_validator.node_keypair.pubkey()),
            );
            assert_eq!(
                if deactivate_feature {
                    RentState::RentPaying {
                        lamports: old_rent_paying_validator_lamports + RENT_PER_VALIDATOR,
                        data_size: 0,
                    }
                } else {
                    RentState::RentPaying {
                        lamports: old_rent_paying_validator_lamports,
                        data_size: 0,
                    }
                },
                get_rent_state(&bank, &rent_paying_validator.node_keypair.pubkey()),
            );
            assert_eq!(
                RentState::RentExempt,
                get_rent_state(&bank, &becomes_rent_exempt_validator.node_keypair.pubkey()),
            );
            assert_eq!(
                RentState::RentExempt,
                get_rent_state(&bank, &rent_exempt_validator.node_keypair.pubkey()),
            );
        }
    }

    #[test]
    fn test_distribute_rent_to_validators_invalid_owner() {
        struct TestCase {
            disable_owner_check: bool,
            use_invalid_owner: bool,
        }

        impl TestCase {
            fn new(disable_owner_check: bool, use_invalid_owner: bool) -> Self {
                Self {
                    disable_owner_check,
                    use_invalid_owner,
                }
            }
        }

        for test_case in [
            TestCase::new(false, false),
            TestCase::new(false, true),
            TestCase::new(true, false),
            TestCase::new(true, true),
        ] {
            let genesis_config_info =
                create_genesis_config_with_leader(0, &Pubkey::new_unique(), 100);
            let mut genesis_config = genesis_config_info.genesis_config;
            genesis_config.rent = Rent::default(); // Ensure rent is non-zero, as genesis_utils sets Rent::free by default

            if test_case.disable_owner_check {
                genesis_config
                    .accounts
                    .remove(&feature_set::validate_fee_collector_account::id())
                    .unwrap();
            }
            let bank = Bank::new_for_tests(&genesis_config);

            let initial_balance = 1_000_000;
            let account_owner = if test_case.use_invalid_owner {
                Pubkey::new_unique()
            } else {
                system_program::id()
            };
            let account = AccountSharedData::new(initial_balance, 0, &account_owner);
            bank.store_account(bank.collector_id(), &account);

            let initial_capitalization = bank.capitalization();
            let rent_fees = 100;
            bank.distribute_rent_to_validators(&bank.vote_accounts(), rent_fees);
            let new_capitalization = bank.capitalization();
            let new_balance = bank.get_balance(bank.collector_id());

            if test_case.use_invalid_owner && !test_case.disable_owner_check {
                assert_eq!(initial_balance, new_balance);
                assert_eq!(initial_capitalization - rent_fees, new_capitalization);
                assert_eq!(bank.rewards.read().unwrap().len(), 0);
            } else {
                assert_eq!(initial_balance + rent_fees, new_balance);
                assert_eq!(initial_capitalization, new_capitalization);
                assert_eq!(bank.rewards.read().unwrap().len(), 1);
            }
        }
    }
}
