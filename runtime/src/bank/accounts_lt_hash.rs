use {
    super::Bank,
    rayon::prelude::*,
    solana_accounts_db::accounts_db::AccountsDb,
    solana_lattice_hash::lt_hash::LtHash,
    solana_measure::{meas_dur, measure::Measure},
    solana_sdk::{
        account::{accounts_equal, AccountSharedData},
        pubkey::Pubkey,
    },
    solana_svm::transaction_processing_callback::AccountState,
    std::{ops::AddAssign, time::Duration},
};

impl Bank {
    /// Returns if the accounts lt hash is enabled
    pub fn is_accounts_lt_hash_enabled(&self) -> bool {
        self.rc
            .accounts
            .accounts_db
            .is_experimental_accumulator_hash_enabled()
    }

    /// Updates the accounts lt hash
    ///
    /// When freezing a bank, we compute and update the accounts lt hash.
    /// For each account modified in this bank, we:
    /// - mix out its previous state, and
    /// - mix in its current state
    ///
    /// Since this function is non-idempotent, it should only be called once per bank.
    pub fn update_accounts_lt_hash(&self) {
        debug_assert!(self.is_accounts_lt_hash_enabled());
        let delta_lt_hash = self.calculate_delta_lt_hash();
        let mut accounts_lt_hash = self.accounts_lt_hash.lock().unwrap();
        accounts_lt_hash.0.mix_in(&delta_lt_hash);
    }

    /// Calculates the lt hash *of only this slot*
    ///
    /// This can be thought of as akin to the accounts delta hash.
    ///
    /// For each account modified in this bank, we:
    /// - mix out its previous state, and
    /// - mix in its current state
    ///
    /// This function is idempotent, and may be called more than once.
    fn calculate_delta_lt_hash(&self) -> LtHash {
        debug_assert!(self.is_accounts_lt_hash_enabled());
        let measure_total = Measure::start("");
        let slot = self.slot();

        // If we don't find the account in the cache, we need to go load it.
        // We want the version of the account *before* it was written in this slot.
        // Bank::ancestors *includes* this slot, so we need to remove it before loading.
        let strictly_ancestors = {
            let mut ancestors = self.ancestors.clone();
            ancestors.remove(&self.slot());
            ancestors
        };

        if slot == 0 {
            // Slot 0 is special when calculating the accounts lt hash.
            // Primordial accounts (those in genesis) that are modified by transaction processing
            // in slot 0 will have Alive entries in the accounts lt hash cache.
            // When calculating the accounts lt hash, if an account was initially alive, we mix
            // *out* its previous lt hash value.  In slot 0, we haven't stored any previous lt hash
            // values (since it is in the first slot), yet we'd still mix out these accounts!
            // This produces the incorrect accounts lt hash.
            // From the perspective of the accounts lt hash, in slot 0 we cannot have any accounts
            // as previously alive.  So to work around this issue, we clear the cache.
            // And since `strictly_ancestors` is empty, loading the previous version of the account
            // from accounts db will return `None` (aka Dead), which is the correct behavior.
            assert!(strictly_ancestors.is_empty());
            self.cache_for_accounts_lt_hash.write().unwrap().clear();
        }

        // Get all the accounts stored in this slot.
        // Since this bank is in the middle of being frozen, it hasn't been rooted.
        // That means the accounts should all be in the write cache, and loading will be fast.
        let (accounts_curr, time_loading_accounts_curr) = meas_dur!({
            self.rc
                .accounts
                .accounts_db
                .get_pubkey_hash_account_for_slot(slot)
        });
        let num_accounts_total = accounts_curr.len();

        #[derive(Debug, Default)]
        struct Stats {
            num_cache_misses: usize,
            num_accounts_unmodified: usize,
            time_loading_accounts_prev: Duration,
            time_comparing_accounts: Duration,
            time_computing_hashes: Duration,
            time_mixing_hashes: Duration,
        }
        impl AddAssign for Stats {
            fn add_assign(&mut self, other: Self) {
                self.num_cache_misses += other.num_cache_misses;
                self.num_accounts_unmodified += other.num_accounts_unmodified;
                self.time_loading_accounts_prev += other.time_loading_accounts_prev;
                self.time_comparing_accounts += other.time_comparing_accounts;
                self.time_computing_hashes += other.time_computing_hashes;
                self.time_mixing_hashes += other.time_mixing_hashes;
            }
        }

        let do_calculate_delta_lt_hash = || {
            // Work on chunks of 128 pubkeys, which is 4 KiB.
            // And 4 KiB is likely the smallest a real page size will be.
            // And a single page is likely the smallest size a disk read will actually read.
            // This can be tuned larger, but likely not smaller.
            const CHUNK_SIZE: usize = 128;
            let cache_for_accounts_lt_hash = self.cache_for_accounts_lt_hash.read().unwrap();
            accounts_curr
                .par_iter()
                .fold_chunks(
                    CHUNK_SIZE,
                    || (LtHash::identity(), Stats::default()),
                    |mut accum, elem| {
                        let pubkey = &elem.pubkey;
                        let curr_account = &elem.account;

                        // load the initial state of the account
                        let (initial_state_of_account, measure_load) = meas_dur!({
                            match cache_for_accounts_lt_hash.get(pubkey) {
                                Some(initial_state_of_account) => initial_state_of_account.clone(),
                                None => {
                                    accum.1.num_cache_misses += 1;
                                    // If the initial state of the account is not in the accounts
                                    // lt hash cache, it is likely this account was stored
                                    // *outside* of transaction processing (e.g. as part of rent
                                    // collection).  Do not populate the read cache, as this
                                    // account likely will not be accessed again soon.
                                    let account_slot = self
                                        .rc
                                        .accounts
                                        .load_with_fixed_root_do_not_populate_read_cache(
                                            &strictly_ancestors,
                                            pubkey,
                                        );
                                    match account_slot {
                                        Some((account, _slot)) => {
                                            InitialStateOfAccount::Alive(account)
                                        }
                                        None => InitialStateOfAccount::Dead,
                                    }
                                }
                            }
                        });
                        accum.1.time_loading_accounts_prev += measure_load;

                        // mix out the previous version of the account
                        match initial_state_of_account {
                            InitialStateOfAccount::Dead => {
                                // nothing to do here
                            }
                            InitialStateOfAccount::Alive(prev_account) => {
                                let (are_accounts_equal, measure_is_equal) =
                                    meas_dur!(accounts_equal(curr_account, &prev_account));
                                accum.1.time_comparing_accounts += measure_is_equal;
                                if are_accounts_equal {
                                    // this account didn't actually change, so skip it for lt hashing
                                    accum.1.num_accounts_unmodified += 1;
                                    return accum;
                                }
                                let (prev_lt_hash, measure_hashing) =
                                    meas_dur!(AccountsDb::lt_hash_account(&prev_account, pubkey));
                                let (_, measure_mixing) =
                                    meas_dur!(accum.0.mix_out(&prev_lt_hash.0));
                                accum.1.time_computing_hashes += measure_hashing;
                                accum.1.time_mixing_hashes += measure_mixing;
                            }
                        }

                        // mix in the new version of the account
                        let (curr_lt_hash, measure_hashing) =
                            meas_dur!(AccountsDb::lt_hash_account(curr_account, pubkey));
                        let (_, measure_mixing) = meas_dur!(accum.0.mix_in(&curr_lt_hash.0));
                        accum.1.time_computing_hashes += measure_hashing;
                        accum.1.time_mixing_hashes += measure_mixing;

                        accum
                    },
                )
                .reduce(
                    || (LtHash::identity(), Stats::default()),
                    |mut accum, elem| {
                        accum.0.mix_in(&elem.0);
                        accum.1 += elem.1;
                        accum
                    },
                )
        };
        let (delta_lt_hash, stats) = self
            .rc
            .accounts
            .accounts_db
            .thread_pool
            .install(do_calculate_delta_lt_hash);

        let total_time = measure_total.end_as_duration();
        let num_accounts_modified =
            num_accounts_total.saturating_sub(stats.num_accounts_unmodified);
        datapoint_info!(
            "bank-accounts_lt_hash",
            ("slot", slot, i64),
            ("num_accounts_total", num_accounts_total, i64),
            ("num_accounts_modified", num_accounts_modified, i64),
            (
                "num_accounts_unmodified",
                stats.num_accounts_unmodified,
                i64
            ),
            ("num_cache_misses", stats.num_cache_misses, i64),
            ("total_us", total_time.as_micros(), i64),
            (
                "loading_accounts_curr_us",
                time_loading_accounts_curr.as_micros(),
                i64
            ),
            (
                "par_loading_accounts_prev_us",
                stats.time_loading_accounts_prev.as_micros(),
                i64
            ),
            (
                "par_comparing_accounts_us",
                stats.time_comparing_accounts.as_micros(),
                i64
            ),
            (
                "par_computing_hashes_us",
                stats.time_computing_hashes.as_micros(),
                i64
            ),
            (
                "par_mixing_hashes_us",
                stats.time_mixing_hashes.as_micros(),
                i64
            ),
        );

        delta_lt_hash
    }

    /// Caches initial state of writeable accounts
    ///
    /// If a transaction account is writeable, cache its initial account state.
    /// The initial state is needed when computing the accounts lt hash for the slot, and caching
    /// the initial state saves us from having to look it up on disk later.
    pub fn inspect_account_for_accounts_lt_hash(
        &self,
        address: &Pubkey,
        account_state: &AccountState,
        is_writable: bool,
    ) {
        debug_assert!(self.is_accounts_lt_hash_enabled());
        if !is_writable {
            // if the account is not writable, then it cannot be modified; nothing to do here
            return;
        }

        // Only insert the account the *first* time we see it.
        // We want to capture the value of the account *before* any modifications during this slot.
        let is_in_cache = self
            .cache_for_accounts_lt_hash
            .read()
            .unwrap()
            .contains_key(address);
        if !is_in_cache {
            self.cache_for_accounts_lt_hash
                .write()
                .unwrap()
                .entry(*address)
                .or_insert_with(|| match account_state {
                    AccountState::Dead => InitialStateOfAccount::Dead,
                    AccountState::Alive(account) => {
                        InitialStateOfAccount::Alive((*account).clone())
                    }
                });
        }
    }
}

/// The initial state of an account prior to being modified in this slot/transaction
#[derive(Debug, Clone)]
pub enum InitialStateOfAccount {
    /// The account was initiall dead
    Dead,
    /// The account was initially alive
    Alive(AccountSharedData),
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            bank::tests::new_bank_from_parent_with_bank_forks, genesis_utils,
            runtime_config::RuntimeConfig, snapshot_bank_utils, snapshot_config::SnapshotConfig,
            snapshot_utils,
        },
        solana_accounts_db::accounts_db::{
            AccountsDbConfig, DuplicatesLtHash, ACCOUNTS_DB_CONFIG_FOR_TESTING,
        },
        solana_sdk::{
            account::{ReadableAccount as _, WritableAccount as _},
            fee_calculator::FeeRateGovernor,
            genesis_config::{self, GenesisConfig},
            native_token::LAMPORTS_PER_SOL,
            pubkey::{self, Pubkey},
            signature::Signer as _,
            signer::keypair::Keypair,
        },
        std::{cmp, collections::HashMap, ops::RangeFull, str::FromStr as _, sync::Arc},
        tempfile::TempDir,
        test_case::test_case,
    };

    /// What features should be enabled?
    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    enum Features {
        /// Do not enable any features
        None,
        /// Enable all features
        All,
    }

    /// Creates a genesis config with `features` enabled
    fn genesis_config_with(features: Features) -> (GenesisConfig, Keypair) {
        let mint_lamports = 123_456_789 * LAMPORTS_PER_SOL;
        match features {
            Features::None => genesis_config::create_genesis_config(mint_lamports),
            Features::All => {
                let info = genesis_utils::create_genesis_config(mint_lamports);
                (info.genesis_config, info.mint_keypair)
            }
        }
    }

    #[test]
    fn test_update_accounts_lt_hash() {
        // Write to address 1, 2, and 5 in first bank, so that in second bank we have
        // updates to these three accounts.  Make address 2 go to zero (dead).  Make address 1 and 3 stay
        // alive.  Make address 5 unchanged.  Ensure the updates are expected.
        //
        // 1: alive -> alive
        // 2: alive -> dead
        // 3: dead -> alive
        // 4. dead -> dead
        // 5. alive -> alive *unchanged*

        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();
        let keypair5 = Keypair::new();

        let (mut genesis_config, mint_keypair) =
            genesis_config::create_genesis_config(123_456_789 * LAMPORTS_PER_SOL);
        genesis_config.fee_rate_governor = FeeRateGovernor::new(0, 0);
        let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        bank.rc
            .accounts
            .accounts_db
            .set_is_experimental_accumulator_hash_enabled(true);

        // ensure the accounts lt hash is enabled, otherwise this test doesn't actually do anything...
        assert!(bank.is_accounts_lt_hash_enabled());

        let amount = cmp::max(
            bank.get_minimum_balance_for_rent_exemption(0),
            LAMPORTS_PER_SOL,
        );

        // send lamports to accounts 1, 2, and 5 so they are alive,
        // and so we'll have a delta in the next bank
        bank.register_unique_recent_blockhash_for_test();
        bank.transfer(amount, &mint_keypair, &keypair1.pubkey())
            .unwrap();
        bank.transfer(amount, &mint_keypair, &keypair2.pubkey())
            .unwrap();
        bank.transfer(amount, &mint_keypair, &keypair5.pubkey())
            .unwrap();

        // manually freeze the bank to trigger update_accounts_lt_hash() to run
        bank.freeze();
        let prev_accounts_lt_hash = bank.accounts_lt_hash.lock().unwrap().clone();

        // save the initial values of the accounts to use for asserts later
        let prev_mint = bank.get_account_with_fixed_root(&mint_keypair.pubkey());
        let prev_account1 = bank.get_account_with_fixed_root(&keypair1.pubkey());
        let prev_account2 = bank.get_account_with_fixed_root(&keypair2.pubkey());
        let prev_account3 = bank.get_account_with_fixed_root(&keypair3.pubkey());
        let prev_account4 = bank.get_account_with_fixed_root(&keypair4.pubkey());
        let prev_account5 = bank.get_account_with_fixed_root(&keypair5.pubkey());

        assert!(prev_mint.is_some());
        assert!(prev_account1.is_some());
        assert!(prev_account2.is_some());
        assert!(prev_account3.is_none());
        assert!(prev_account4.is_none());
        assert!(prev_account5.is_some());

        // These sysvars are also updated, but outside of transaction processing.  This means they
        // will not be in the accounts lt hash cache, but *will* be in the list of modified
        // accounts.  They must be included in the accounts lt hash.
        let sysvars = [
            Pubkey::from_str("SysvarS1otHashes111111111111111111111111111").unwrap(),
            Pubkey::from_str("SysvarC1ock11111111111111111111111111111111").unwrap(),
            Pubkey::from_str("SysvarRecentB1ockHashes11111111111111111111").unwrap(),
            Pubkey::from_str("SysvarS1otHistory11111111111111111111111111").unwrap(),
        ];
        let prev_sysvar_accounts: Vec<_> = sysvars
            .iter()
            .map(|address| bank.get_account_with_fixed_root(address))
            .collect();

        let bank = {
            let slot = bank.slot() + 1;
            new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), slot)
        };

        // send from account 2 to account 1; account 1 stays alive, account 2 ends up dead
        bank.register_unique_recent_blockhash_for_test();
        bank.transfer(amount, &keypair2, &keypair1.pubkey())
            .unwrap();

        // send lamports to account 4, then turn around and send them to account 3
        // account 3 will be alive, and account 4 will end dead
        bank.register_unique_recent_blockhash_for_test();
        bank.transfer(amount, &mint_keypair, &keypair4.pubkey())
            .unwrap();
        bank.register_unique_recent_blockhash_for_test();
        bank.transfer(amount, &keypair4, &keypair3.pubkey())
            .unwrap();

        // store account 5 into this new bank, unchanged
        bank.rc.accounts.store_cached(
            (
                bank.slot(),
                [(&keypair5.pubkey(), &prev_account5.clone().unwrap())].as_slice(),
            ),
            None,
        );

        // freeze the bank to trigger update_accounts_lt_hash() to run
        bank.freeze();

        let actual_delta_lt_hash = bank.calculate_delta_lt_hash();
        let post_accounts_lt_hash = bank.accounts_lt_hash.lock().unwrap().clone();
        let post_mint = bank.get_account_with_fixed_root(&mint_keypair.pubkey());
        let post_account1 = bank.get_account_with_fixed_root(&keypair1.pubkey());
        let post_account2 = bank.get_account_with_fixed_root(&keypair2.pubkey());
        let post_account3 = bank.get_account_with_fixed_root(&keypair3.pubkey());
        let post_account4 = bank.get_account_with_fixed_root(&keypair4.pubkey());
        let post_account5 = bank.get_account_with_fixed_root(&keypair5.pubkey());

        assert!(post_mint.is_some());
        assert!(post_account1.is_some());
        assert!(post_account2.is_none());
        assert!(post_account3.is_some());
        assert!(post_account4.is_none());
        assert!(post_account5.is_some());

        let post_sysvar_accounts: Vec<_> = sysvars
            .iter()
            .map(|address| bank.get_account_with_fixed_root(address))
            .collect();

        let mut expected_delta_lt_hash = LtHash::identity();
        let mut expected_accounts_lt_hash = prev_accounts_lt_hash.clone();
        let mut updater =
            |address: &Pubkey, prev: Option<AccountSharedData>, post: Option<AccountSharedData>| {
                // if there was an alive account, mix out
                if let Some(prev) = prev {
                    let prev_lt_hash = AccountsDb::lt_hash_account(&prev, address);
                    expected_delta_lt_hash.mix_out(&prev_lt_hash.0);
                    expected_accounts_lt_hash.0.mix_out(&prev_lt_hash.0);
                }

                // mix in the new one
                let post = post.unwrap_or_default();
                let post_lt_hash = AccountsDb::lt_hash_account(&post, address);
                expected_delta_lt_hash.mix_in(&post_lt_hash.0);
                expected_accounts_lt_hash.0.mix_in(&post_lt_hash.0);
            };
        updater(&mint_keypair.pubkey(), prev_mint, post_mint);
        updater(&keypair1.pubkey(), prev_account1, post_account1);
        updater(&keypair2.pubkey(), prev_account2, post_account2);
        updater(&keypair3.pubkey(), prev_account3, post_account3);
        updater(&keypair4.pubkey(), prev_account4, post_account4);
        updater(&keypair5.pubkey(), prev_account5, post_account5);
        for (i, sysvar) in sysvars.iter().enumerate() {
            updater(
                sysvar,
                prev_sysvar_accounts[i].clone(),
                post_sysvar_accounts[i].clone(),
            );
        }

        // now make sure the delta lt hashes match
        let expected = expected_delta_lt_hash.checksum();
        let actual = actual_delta_lt_hash.checksum();
        assert_eq!(
            expected, actual,
            "delta_lt_hash, expected: {expected}, actual: {actual}",
        );

        // ...and the accounts lt hashes match too
        let expected = expected_accounts_lt_hash.0.checksum();
        let actual = post_accounts_lt_hash.0.checksum();
        assert_eq!(
            expected, actual,
            "accounts_lt_hash, expected: {expected}, actual: {actual}",
        );
    }

    /// Ensure that the accounts lt hash is correct for slot 0
    ///
    /// This test does a simple transfer in slot 0 so that a primordial account is modified.
    ///
    /// See the comments in calculate_delta_lt_hash() for more information.
    #[test_case(Features::None; "no features")]
    #[test_case(Features::All; "all features")]
    fn test_slot0_accounts_lt_hash(features: Features) {
        let (genesis_config, mint_keypair) = genesis_config_with(features);
        let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        bank.rc
            .accounts
            .accounts_db
            .set_is_experimental_accumulator_hash_enabled(true);

        // ensure the accounts lt hash is enabled, otherwise this test doesn't actually do anything...
        assert!(bank.is_accounts_lt_hash_enabled());

        // ensure this bank is for slot 0, otherwise this test doesn't actually do anything...
        assert_eq!(bank.slot(), 0);

        // process a transaction that modifies a primordial account
        bank.transfer(LAMPORTS_PER_SOL, &mint_keypair, &Pubkey::new_unique())
            .unwrap();

        // manually freeze the bank to trigger update_accounts_lt_hash() to run
        bank.freeze();
        let actual_accounts_lt_hash = bank.accounts_lt_hash.lock().unwrap().clone();

        // ensure the actual accounts lt hash matches the value calculated from the index
        let calculated_accounts_lt_hash = bank
            .rc
            .accounts
            .accounts_db
            .calculate_accounts_lt_hash_at_startup_from_index(&bank.ancestors, bank.slot());
        assert_eq!(actual_accounts_lt_hash, calculated_accounts_lt_hash);
    }

    #[test_case(Features::None; "no features")]
    #[test_case(Features::All; "all features")]
    fn test_inspect_account_for_accounts_lt_hash(features: Features) {
        let (genesis_config, _mint_keypair) = genesis_config_with(features);
        let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        bank.rc
            .accounts
            .accounts_db
            .set_is_experimental_accumulator_hash_enabled(true);

        // ensure the accounts lt hash is enabled, otherwise this test doesn't actually do anything...
        assert!(bank.is_accounts_lt_hash_enabled());

        // the cache should start off empty
        assert_eq!(bank.cache_for_accounts_lt_hash.read().unwrap().len(), 0);

        // ensure non-writable accounts are *not* added to the cache
        bank.inspect_account_for_accounts_lt_hash(
            &Pubkey::new_unique(),
            &AccountState::Dead,
            false,
        );
        bank.inspect_account_for_accounts_lt_hash(
            &Pubkey::new_unique(),
            &AccountState::Alive(&AccountSharedData::default()),
            false,
        );
        assert_eq!(bank.cache_for_accounts_lt_hash.read().unwrap().len(), 0);

        // ensure *new* accounts are added to the cache
        let address = Pubkey::new_unique();
        bank.inspect_account_for_accounts_lt_hash(&address, &AccountState::Dead, true);
        assert_eq!(bank.cache_for_accounts_lt_hash.read().unwrap().len(), 1);
        assert!(bank
            .cache_for_accounts_lt_hash
            .read()
            .unwrap()
            .contains_key(&address));

        // ensure *existing* accounts are added to the cache
        let address = Pubkey::new_unique();
        let initial_lamports = 123;
        let mut account = AccountSharedData::new(initial_lamports, 0, &Pubkey::default());
        bank.inspect_account_for_accounts_lt_hash(&address, &AccountState::Alive(&account), true);
        assert_eq!(bank.cache_for_accounts_lt_hash.read().unwrap().len(), 2);
        if let InitialStateOfAccount::Alive(cached_account) = bank
            .cache_for_accounts_lt_hash
            .read()
            .unwrap()
            .get(&address)
            .unwrap()
        {
            assert_eq!(*cached_account, account);
        } else {
            panic!("wrong initial state for account");
        };

        // ensure if an account is modified multiple times that we only cache the *first* one
        let updated_lamports = account.lamports() + 1;
        account.set_lamports(updated_lamports);
        bank.inspect_account_for_accounts_lt_hash(&address, &AccountState::Alive(&account), true);
        assert_eq!(bank.cache_for_accounts_lt_hash.read().unwrap().len(), 2);
        if let InitialStateOfAccount::Alive(cached_account) = bank
            .cache_for_accounts_lt_hash
            .read()
            .unwrap()
            .get(&address)
            .unwrap()
        {
            assert_eq!(cached_account.lamports(), initial_lamports);
        } else {
            panic!("wrong initial state for account");
        };

        // and ensure multiple updates are handled correctly when the account is initially dead
        {
            let address = Pubkey::new_unique();
            bank.inspect_account_for_accounts_lt_hash(&address, &AccountState::Dead, true);
            assert_eq!(bank.cache_for_accounts_lt_hash.read().unwrap().len(), 3);
            match bank
                .cache_for_accounts_lt_hash
                .read()
                .unwrap()
                .get(&address)
                .unwrap()
            {
                InitialStateOfAccount::Dead => { /* this is expected, nothing to do here*/ }
                _ => panic!("wrong initial state for account"),
            };

            bank.inspect_account_for_accounts_lt_hash(
                &address,
                &AccountState::Alive(&AccountSharedData::default()),
                true,
            );
            assert_eq!(bank.cache_for_accounts_lt_hash.read().unwrap().len(), 3);
            match bank
                .cache_for_accounts_lt_hash
                .read()
                .unwrap()
                .get(&address)
                .unwrap()
            {
                InitialStateOfAccount::Dead => { /* this is expected, nothing to do here*/ }
                _ => panic!("wrong initial state for account"),
            };
        }
    }

    #[test_case(Features::None; "no features")]
    #[test_case(Features::All; "all features")]
    fn test_calculate_accounts_lt_hash_at_startup_from_index(features: Features) {
        let (genesis_config, mint_keypair) = genesis_config_with(features);
        let (mut bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        bank.rc
            .accounts
            .accounts_db
            .set_is_experimental_accumulator_hash_enabled(true);

        // ensure the accounts lt hash is enabled, otherwise this test doesn't actually do anything...
        assert!(bank.is_accounts_lt_hash_enabled());

        let amount = cmp::max(
            bank.get_minimum_balance_for_rent_exemption(0),
            LAMPORTS_PER_SOL,
        );

        // create some banks with some modified accounts so that there are stored accounts
        // (note: the number of banks and transfers are arbitrary)
        for _ in 0..7 {
            let slot = bank.slot() + 1;
            bank =
                new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), slot);
            for _ in 0..13 {
                bank.register_unique_recent_blockhash_for_test();
                // note: use a random pubkey here to ensure accounts
                // are spread across all the index bins
                bank.transfer(amount, &mint_keypair, &pubkey::new_rand())
                    .unwrap();
            }
            bank.freeze();
        }
        let expected_accounts_lt_hash = bank.accounts_lt_hash.lock().unwrap().clone();

        // root the bank and flush the accounts write cache to disk
        // (this more accurately simulates startup, where accounts are in storages on disk)
        bank.squash();
        bank.force_flush_accounts_cache();

        // call the fn that calculates the accounts lt hash at startup, then ensure it matches
        let calculated_accounts_lt_hash = bank
            .rc
            .accounts
            .accounts_db
            .calculate_accounts_lt_hash_at_startup_from_index(&bank.ancestors, bank.slot());
        assert_eq!(expected_accounts_lt_hash, calculated_accounts_lt_hash);
    }

    #[test_case(Features::None; "no features")]
    #[test_case(Features::All; "all features")]
    fn test_calculate_accounts_lt_hash_at_startup_from_storages(features: Features) {
        let (genesis_config, mint_keypair) = genesis_config_with(features);
        let (mut bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        bank.rc
            .accounts
            .accounts_db
            .set_is_experimental_accumulator_hash_enabled(true);

        // ensure the accounts lt hash is enabled, otherwise this test doesn't actually do anything...
        assert!(bank.is_accounts_lt_hash_enabled());

        let amount = cmp::max(
            bank.get_minimum_balance_for_rent_exemption(0),
            LAMPORTS_PER_SOL,
        );

        // Write to this pubkey multiple times, so there are guaranteed duplicates in the storages.
        let duplicate_pubkey = pubkey::new_rand();

        // create some banks with some modified accounts so that there are stored accounts
        // (note: the number of banks and transfers are arbitrary)
        for _ in 0..7 {
            let slot = bank.slot() + 1;
            bank =
                new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), slot);
            for _ in 0..9 {
                bank.register_unique_recent_blockhash_for_test();
                // note: use a random pubkey here to ensure accounts
                // are spread across all the index bins
                // (and calculating the accounts lt hash from storages requires no duplicates)
                bank.transfer(amount, &mint_keypair, &pubkey::new_rand())
                    .unwrap();

                bank.register_unique_recent_blockhash_for_test();
                bank.transfer(amount, &mint_keypair, &duplicate_pubkey)
                    .unwrap();
            }

            // flush the write cache each slot to ensure there are account duplicates in the storages
            bank.squash();
            bank.force_flush_accounts_cache();
        }
        let expected_accounts_lt_hash = bank.accounts_lt_hash.lock().unwrap().clone();

        // go through the storages to find the duplicates
        let (mut storages, _slots) = bank
            .rc
            .accounts
            .accounts_db
            .get_snapshot_storages(RangeFull);
        // sort the storages in slot-descending order
        // this makes skipping the latest easier
        storages.sort_unstable_by_key(|storage| cmp::Reverse(storage.slot()));
        let storages = storages.into_boxed_slice();

        // get all the lt hashes for each version of all accounts
        let mut stored_accounts_map = HashMap::<_, Vec<_>>::new();
        for storage in &storages {
            storage.accounts.scan_accounts(|stored_account_meta| {
                let pubkey = stored_account_meta.pubkey();
                let account_lt_hash = AccountsDb::lt_hash_account(&stored_account_meta, pubkey);
                stored_accounts_map
                    .entry(*pubkey)
                    .or_default()
                    .push(account_lt_hash)
            });
        }

        // calculate the duplicates lt hash by skipping the first version (latest) of each account,
        // and then mixing together all the rest
        let duplicates_lt_hash = stored_accounts_map
            .values()
            .map(|lt_hashes| {
                // the first element in the vec is the latest; all the rest are duplicates
                &lt_hashes[1..]
            })
            .fold(LtHash::identity(), |mut accum, duplicate_lt_hashes| {
                for duplicate_lt_hash in duplicate_lt_hashes {
                    accum.mix_in(&duplicate_lt_hash.0);
                }
                accum
            });
        let duplicates_lt_hash = DuplicatesLtHash(duplicates_lt_hash);

        // ensure that calculating the accounts lt hash from storages is correct
        let calculated_accounts_lt_hash_from_storages = bank
            .rc
            .accounts
            .accounts_db
            .calculate_accounts_lt_hash_at_startup_from_storages(&storages, &duplicates_lt_hash);
        assert_eq!(
            expected_accounts_lt_hash,
            calculated_accounts_lt_hash_from_storages
        );
    }

    #[test_case(Features::None; "no features")]
    #[test_case(Features::All; "all features")]
    fn test_verify_accounts_lt_hash_at_startup(features: Features) {
        let (genesis_config, mint_keypair) = genesis_config_with(features);
        let (mut bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        bank.rc
            .accounts
            .accounts_db
            .set_is_experimental_accumulator_hash_enabled(true);

        // ensure the accounts lt hash is enabled, otherwise this test doesn't actually do anything...
        assert!(bank.is_accounts_lt_hash_enabled());

        let amount = cmp::max(
            bank.get_minimum_balance_for_rent_exemption(0),
            LAMPORTS_PER_SOL,
        );

        // Write to this pubkey multiple times, so there are guaranteed duplicates in the storages.
        let duplicate_pubkey = pubkey::new_rand();

        // create some banks with some modified accounts so that there are stored accounts
        // (note: the number of banks and transfers are arbitrary)
        for _ in 0..9 {
            let slot = bank.slot() + 1;
            bank =
                new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), slot);
            for _ in 0..3 {
                bank.register_unique_recent_blockhash_for_test();
                bank.transfer(amount, &mint_keypair, &pubkey::new_rand())
                    .unwrap();
                bank.register_unique_recent_blockhash_for_test();
                bank.transfer(amount, &mint_keypair, &duplicate_pubkey)
                    .unwrap();
            }

            // flush the write cache to disk to ensure there are duplicates across the storages
            bank.fill_bank_with_ticks_for_tests();
            bank.squash();
            bank.force_flush_accounts_cache();
        }

        // verification happens at startup, so mimic the behavior by loading from a snapshot
        let snapshot_config = SnapshotConfig::default();
        let bank_snapshots_dir = TempDir::new().unwrap();
        let snapshot_archives_dir = TempDir::new().unwrap();
        let snapshot = snapshot_bank_utils::bank_to_full_snapshot_archive(
            &bank_snapshots_dir,
            &bank,
            Some(snapshot_config.snapshot_version),
            &snapshot_archives_dir,
            &snapshot_archives_dir,
            snapshot_config.archive_format,
        )
        .unwrap();
        let (_accounts_tempdir, accounts_dir) = snapshot_utils::create_tmp_accounts_dir_for_tests();
        let accounts_db_config = AccountsDbConfig {
            enable_experimental_accumulator_hash: true,
            ..ACCOUNTS_DB_CONFIG_FOR_TESTING
        };
        let (roundtrip_bank, _) = snapshot_bank_utils::bank_from_snapshot_archives(
            &[accounts_dir],
            &bank_snapshots_dir,
            &snapshot,
            None,
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            None,
            false,
            false,
            false,
            false,
            Some(accounts_db_config),
            None,
            Arc::default(),
        )
        .unwrap();

        // Wait for the startup verification to complete.  If we don't panic, then we're good!
        roundtrip_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(roundtrip_bank, *bank);
    }
}
