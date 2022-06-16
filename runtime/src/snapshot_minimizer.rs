use std::{collections::HashSet, sync::Arc};

use dashmap::DashSet;
use log::info;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use solana_measure::measure;
use solana_sdk::{
    account::ReadableAccount,
    account_utils::StateMut,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    clock::Slot,
    pubkey::Pubkey,
};

use crate::{accounts_index::ScanConfig, bank::Bank};

struct SnapshotMinimizer {
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
    }

    /// Create a minimizer - used in tests
    pub(self) fn new(
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
        let (_, measure) = measure!(|| add_accounts_fn(self), name);
        let total_accounts_len = self.minimized_account_set.len();
        let added_accounts = total_accounts_len - initial_accounts_len;

        info!(
            "Added {added_accounts} {name} for total of {total_accounts_len} accounts. get {measure}"
        );
    }

    /// Used to get rent collection accounts in `minimize`
    pub(self) fn get_rent_collection_accounts(&self) {
        self.bank.get_rent_collection_accounts_between_slots(
            &self.minimized_account_set,
            self.starting_slot,
            self.ending_slot,
        );
    }

    /// Used to get vote and node pubkeys in `minimize`
    pub(self) fn get_vote_accounts(&self) {
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
    pub(self) fn get_stake_accounts(&self) {
        self.bank
            .get_program_accounts(&solana_sdk::stake::program::id(), &ScanConfig::default())
            .unwrap()
            .into_par_iter()
            .for_each(|(pubkey, _)| {
                self.minimized_account_set.insert(pubkey);
            });
    }

    /// Used to get owner accounts in `minimize`
    pub(self) fn get_owner_accounts(&self) {
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
    pub(self) fn get_programdata_accounts(&self) {
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dashmap::DashSet;
    use solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        genesis_config::create_genesis_config,
        pubkey::Pubkey,
        signer::Signer,
        stake,
    };

    use crate::{
        bank::Bank, genesis_utils::create_genesis_config_with_leader,
        snapshot_minimizer::SnapshotMinimizer,
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

        let expected_stake_accounts: Vec<_> = genesis_config_info
            .genesis_config
            .accounts
            .iter()
            .filter_map(|(pubkey, account)| {
                stake::program::check_id(account.owner()).then(|| *pubkey)
            })
            .collect();
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
}
