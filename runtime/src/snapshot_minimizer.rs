use {
    crate::bank::Bank,
    dashmap::DashSet,
    log::info,
    rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator},
    solana_measure::measure,
    solana_sdk::{
        account::ReadableAccount,
        account_utils::StateMut,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        clock::Slot,
        pubkey::Pubkey,
    },
    std::{
        collections::HashSet,
        sync::{Arc, Mutex},
    },
};

pub struct SnapshotMinimizer {
    bank: Arc<Bank>,
    starting_slot: Slot,
    ending_slot: Slot,
    minimized_account_set: DashSet<Pubkey>,
}

impl SnapshotMinimizer {
    /// Removes all accounts not necessary for replaying slots in the range [starting_slot, ending_slot].
    pub fn minimize(
        bank: Arc<Bank>,
        starting_slot: Slot,
        ending_slot: Slot,
        minimized_account_set: DashSet<Pubkey>,
    ) {
        let minimizer = Self::new(bank, starting_slot, ending_slot, minimized_account_set);

        minimizer.add_accounts(
            Self::get_rent_collection_accounts,
            "rent collection accounts",
        );
        minimizer.add_accounts(Self::get_vote_accounts, "vote accounts");
        minimizer.add_accounts(Self::get_stake_accounts, "stake accounts");
        minimizer.add_accounts(Self::get_owner_accounts, "owner accounts");
        minimizer.add_accounts(Self::get_programdata_accounts, "programdata accounts");

        minimizer.minimize_accounts_db();

        // Update accounts_cache and capitalization
        minimizer.bank.force_flush_accounts_cache();
        minimizer.bank.set_capitalization();
    }

    /// Create a minimizer - used in tests
    fn new(
        bank: Arc<Bank>,
        starting_slot: Slot,
        ending_slot: Slot,
        minimized_account_set: DashSet<Pubkey>,
    ) -> Self {
        Self {
            bank,
            starting_slot,
            ending_slot,
            minimized_account_set,
        }
    }

    /// Helper function to measure time and number of accounts added
    fn add_accounts<F>(&self, add_accounts_fn: F, name: &'static str)
    where
        F: Fn(&SnapshotMinimizer),
    {
        let initial_accounts_len = self.minimized_account_set.len();
        let (_, measure) = measure!(add_accounts_fn(self), name);
        let total_accounts_len = self.minimized_account_set.len();
        let added_accounts = total_accounts_len - initial_accounts_len;

        info!(
            "Added {added_accounts} {name} for total of {total_accounts_len} accounts. get {measure}"
        );
    }

    /// Used to get rent collection accounts in `minimize`
    fn get_rent_collection_accounts(&self) {
        self.bank.get_rent_collection_accounts_between_slots(
            &self.minimized_account_set,
            self.starting_slot,
            self.ending_slot,
        );
    }

    /// Used to get vote and node pubkeys in `minimize`
    fn get_vote_accounts(&self) {
        self.bank
            .vote_accounts()
            .par_iter()
            .for_each(|(pubkey, (_stake, vote_account))| {
                self.minimized_account_set.insert(*pubkey);
                if let Ok(vote_state) = vote_account.vote_state().as_ref() {
                    self.minimized_account_set.insert(vote_state.node_pubkey);
                }
            });
    }

    /// Used to get stake accounts in `minimize`
    fn get_stake_accounts(&self) {
        self.bank.get_stake_accounts(&self.minimized_account_set);
    }

    /// Used to get owner accounts in `minimize`
    fn get_owner_accounts(&self) {
        let owner_accounts: HashSet<_> = self
            .minimized_account_set
            .par_iter()
            .filter_map(|pubkey| self.bank.get_account(&pubkey))
            .map(|account| *account.owner())
            .collect();
        owner_accounts.into_par_iter().for_each(|pubkey| {
            self.minimized_account_set.insert(pubkey);
        });
    }

    /// Used to get program data accounts in `minimize`
    fn get_programdata_accounts(&self) {
        let programdata_accounts: HashSet<_> = self
            .minimized_account_set
            .par_iter()
            .filter_map(|pubkey| self.bank.get_account(&pubkey))
            .filter(|account| account.executable())
            .filter(|account| bpf_loader_upgradeable::check_id(account.owner()))
            .filter_map(|account| {
                if let Ok(UpgradeableLoaderState::Program {
                    programdata_address,
                }) = account.state()
                {
                    Some(programdata_address)
                } else {
                    None
                }
            })
            .collect();
        programdata_accounts.into_par_iter().for_each(|pubkey| {
            self.minimized_account_set.insert(pubkey);
        });
    }

    /// Remove accounts from accounts_db
    fn minimize_accounts_db(&self) {
        let minimized_slot_set = self.get_minimized_slot_set();

        let dead_slots = Mutex::new(Vec::new());
        let dead_storages = Mutex::new(Vec::new());

        let (_, total_filter_storages_measure) = measure!(
            {
                let snapshot_storages = self
                    .bank
                    .accounts()
                    .accounts_db
                    .get_snapshot_storages(self.starting_slot, None, None)
                    .0;
                snapshot_storages.into_par_iter().for_each(|storages| {
                    let slot = storages.first().unwrap().slot();
                    if slot != self.starting_slot {
                        if minimized_slot_set.contains(&slot) {
                            self.bank.accounts().accounts_db.filter_storages(
                                storages,
                                &self.minimized_account_set,
                                &dead_storages,
                            );
                        } else {
                            dead_slots.lock().unwrap().push(slot);
                        }
                    }
                })
            },
            "filter storages total"
        );
        info!("{total_filter_storages_measure}");

        self.bank
            .accounts()
            .accounts_db
            .remove_dead_slots_and_storages(
                dead_slots.into_inner().unwrap(),
                dead_storages.into_inner().unwrap(),
            );
    }

    /// Determines minimum set of slots that accounts in `minimized_account_set` are in
    fn get_minimized_slot_set(&self) -> DashSet<Slot> {
        let (minimized_slot_set, measure) = measure!(
            {
                let minimized_slot_set = DashSet::new();
                self.minimized_account_set.par_iter().for_each(|pubkey| {
                    if let Some(read_entry) = self
                        .bank
                        .accounts()
                        .accounts_db
                        .accounts_index
                        .get_account_read_entry(&pubkey)
                    {
                        if let Some(max_slot) =
                            read_entry.slot_list().iter().map(|(slot, _)| *slot).max()
                        {
                            minimized_slot_set.insert(max_slot);
                        }
                    }
                });
                minimized_slot_set
            },
            "generate minimized slot set"
        );
        info!("{measure}");

        minimized_slot_set
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            append_vec::AppendVecAccountsIter, bank::Bank,
            genesis_utils::create_genesis_config_with_leader,
            snapshot_minimizer::SnapshotMinimizer,
        },
        dashmap::DashSet,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount, WritableAccount},
            bpf_loader_upgradeable::{self, UpgradeableLoaderState},
            genesis_config::create_genesis_config,
            signer::Signer,
            stake,
        },
        std::sync::Arc,
    };

    #[test]
    fn test_minimization_get_vote_accounts() {
        solana_logger::setup();

        let bootstrap_validator_pubkey = solana_sdk::pubkey::new_rand();
        let bootstrap_validator_stake_lamports = 30;
        let genesis_config_info = create_genesis_config_with_leader(
            10,
            &bootstrap_validator_pubkey,
            bootstrap_validator_stake_lamports,
        );

        let bank = Arc::new(Bank::new_for_tests(&genesis_config_info.genesis_config));

        let minimizer = SnapshotMinimizer::new(bank, 0, 0, DashSet::new());
        minimizer.get_vote_accounts();

        assert!(minimizer
            .minimized_account_set
            .contains(&genesis_config_info.voting_keypair.pubkey()));
        assert!(minimizer
            .minimized_account_set
            .contains(&genesis_config_info.validator_pubkey));
    }

    #[test]
    fn test_minimization_get_stake_accounts() {
        solana_logger::setup();

        let bootstrap_validator_pubkey = solana_sdk::pubkey::new_rand();
        let bootstrap_validator_stake_lamports = 30;
        let genesis_config_info = create_genesis_config_with_leader(
            10,
            &bootstrap_validator_pubkey,
            bootstrap_validator_stake_lamports,
        );

        let bank = Arc::new(Bank::new_for_tests(&genesis_config_info.genesis_config));
        let minimizer = SnapshotMinimizer::new(bank, 0, 0, DashSet::new());
        minimizer.get_stake_accounts();

        let mut expected_stake_accounts: Vec<_> = genesis_config_info
            .genesis_config
            .accounts
            .iter()
            .filter_map(|(pubkey, account)| {
                stake::program::check_id(account.owner()).then(|| *pubkey)
            })
            .collect();
        expected_stake_accounts.push(bootstrap_validator_pubkey);

        assert_eq!(
            minimizer.minimized_account_set.len(),
            expected_stake_accounts.len()
        );
        for stake_pubkey in expected_stake_accounts {
            assert!(minimizer.minimized_account_set.contains(&stake_pubkey));
        }
    }

    #[test]
    fn test_minimization_get_owner_accounts() {
        solana_logger::setup();

        let (genesis_config, _) = create_genesis_config(1_000_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let pubkey = solana_sdk::pubkey::new_rand();
        let owner_pubkey = solana_sdk::pubkey::new_rand();
        bank.store_account(&pubkey, &AccountSharedData::new(1, 0, &owner_pubkey));

        let owner_accounts = DashSet::new();
        owner_accounts.insert(pubkey);
        let minimizer = SnapshotMinimizer::new(bank, 0, 0, owner_accounts);

        minimizer.get_owner_accounts();
        assert!(minimizer.minimized_account_set.contains(&pubkey));
        assert!(minimizer.minimized_account_set.contains(&owner_pubkey));
    }

    #[test]
    fn test_minimization_add_programdata_accounts() {
        solana_logger::setup();

        let (genesis_config, _) = create_genesis_config(1_000_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let non_program_id = solana_sdk::pubkey::new_rand();
        let program_id = solana_sdk::pubkey::new_rand();
        let programdata_address = solana_sdk::pubkey::new_rand();

        let program = UpgradeableLoaderState::Program {
            programdata_address,
        };

        let non_program_acount = AccountSharedData::new(1, 0, &non_program_id);
        let mut program_account =
            AccountSharedData::new_data(40, &program, &bpf_loader_upgradeable::id()).unwrap();
        program_account.set_executable(true);

        bank.store_account(&non_program_id, &non_program_acount);
        bank.store_account(&program_id, &program_account);

        // Non-program account does not add any additional keys
        let programdata_accounts = DashSet::new();
        programdata_accounts.insert(non_program_id);
        let minimizer = SnapshotMinimizer::new(bank, 0, 0, programdata_accounts);
        minimizer.get_programdata_accounts();
        assert_eq!(minimizer.minimized_account_set.len(), 1);
        assert!(minimizer.minimized_account_set.contains(&non_program_id));

        // Programdata account adds the programdata address to the set
        minimizer.minimized_account_set.insert(program_id);
        minimizer.get_programdata_accounts();
        assert_eq!(minimizer.minimized_account_set.len(), 3);
        assert!(minimizer.minimized_account_set.contains(&non_program_id));
        assert!(minimizer.minimized_account_set.contains(&program_id));
        assert!(minimizer
            .minimized_account_set
            .contains(&programdata_address));
    }

    #[test]
    fn test_minimize_accounts_db() {
        solana_logger::setup();

        let (genesis_config, _) = create_genesis_config(1_000_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let accounts = &bank.accounts().accounts_db;

        let num_slots = 5;
        let num_accounts_per_slot = 300;

        let mut current_slot = 0;
        let minimized_account_set = DashSet::new();
        for _ in 0..num_slots {
            let pubkeys: Vec<_> = (0..num_accounts_per_slot)
                .map(|_| solana_sdk::pubkey::new_rand())
                .collect();

            let some_lamport = 223;
            let no_data = 0;
            let owner = *AccountSharedData::default().owner();
            let account = AccountSharedData::new(some_lamport, no_data, &owner);

            current_slot += 1;

            for (index, pubkey) in pubkeys.iter().enumerate() {
                accounts.store_uncached(current_slot, &[(pubkey, &account)]);

                if current_slot % 2 == 0 && index % 100 == 0 {
                    minimized_account_set.insert(*pubkey);
                }
            }
            accounts.get_accounts_delta_hash(current_slot);
            accounts.add_root(current_slot);
        }

        assert_eq!(minimized_account_set.len(), 6);
        let minimizer =
            SnapshotMinimizer::new(bank, current_slot, current_slot, minimized_account_set);
        minimizer.minimize_accounts_db();

        let snapshot_storages = accounts.get_snapshot_storages(current_slot, None, None).0;
        assert_eq!(snapshot_storages.len(), 3);

        let mut account_count = 0;
        snapshot_storages.into_iter().for_each(|storages| {
            storages.into_iter().for_each(|storage| {
                account_count += AppendVecAccountsIter::new(&storage.accounts).count();
            });
        });

        assert_eq!(
            account_count,
            minimizer.minimized_account_set.len() + num_accounts_per_slot
        ); // snapshot slot is untouched, so still has all 300 accounts
    }
}
