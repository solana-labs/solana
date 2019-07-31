//! The `bank_forks` module implments BankForks a DAG of checkpointed Banks

use crate::result::{Error, Result};
use crate::snapshot_package::SnapshotPackageSender;
use crate::snapshot_utils;
use solana_measure::measure::Measure;
use solana_metrics::inc_new_counter_info;
use solana_runtime::bank::{Bank, BankRc, StatusCacheRc};
use solana_runtime::status_cache::MAX_CACHE_ENTRIES;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::timing;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Error as IOError, ErrorKind};
use std::ops::Index;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotConfig {
    snapshot_path: PathBuf,
    snapshot_package_output_path: PathBuf,
    snapshot_interval_slots: usize,
}

impl SnapshotConfig {
    pub fn new(
        snapshot_path: PathBuf,
        snapshot_package_output_path: PathBuf,
        snapshot_interval_slots: usize,
    ) -> Self {
        Self {
            snapshot_path,
            snapshot_package_output_path,
            snapshot_interval_slots,
        }
    }

    pub fn snapshot_path(&self) -> &Path {
        self.snapshot_path.as_path()
    }

    pub fn snapshot_package_output_path(&self) -> &Path {
        &self.snapshot_package_output_path.as_path()
    }

    pub fn snapshot_interval_slots(&self) -> usize {
        self.snapshot_interval_slots
    }
}

pub struct BankForks {
    banks: HashMap<u64, Arc<Bank>>,
    working_bank: Arc<Bank>,
    root: u64,
    snapshot_config: Option<SnapshotConfig>,
    last_snapshot: u64,
    confidence: HashMap<u64, Confidence>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Confidence {
    fork_stakes: u64,
    epoch_stakes: u64,
    lockouts: u64,
    stake_weighted_lockouts: u128,
}

impl Confidence {
    pub fn new(fork_stakes: u64, epoch_stakes: u64, lockouts: u64) -> Self {
        Self {
            fork_stakes,
            epoch_stakes,
            lockouts,
            stake_weighted_lockouts: 0,
        }
    }
    pub fn new_with_stake_weighted(
        fork_stakes: u64,
        epoch_stakes: u64,
        lockouts: u64,
        stake_weighted_lockouts: u128,
    ) -> Self {
        Self {
            fork_stakes,
            epoch_stakes,
            lockouts,
            stake_weighted_lockouts,
        }
    }
}

impl Index<u64> for BankForks {
    type Output = Arc<Bank>;
    fn index(&self, bank_slot: u64) -> &Arc<Bank> {
        &self.banks[&bank_slot]
    }
}

impl BankForks {
    pub fn new(bank_slot: u64, bank: Bank) -> Self {
        let mut banks = HashMap::new();
        let working_bank = Arc::new(bank);
        banks.insert(bank_slot, working_bank.clone());
        Self {
            banks,
            working_bank,
            root: 0,
            snapshot_config: None,
            last_snapshot: 0,
            confidence: HashMap::new(),
        }
    }

    /// Create a map of bank slot id to the set of ancestors for the bank slot.
    pub fn ancestors(&self) -> HashMap<u64, HashSet<u64>> {
        let mut ancestors = HashMap::new();
        for bank in self.banks.values() {
            let mut set: HashSet<u64> = bank.ancestors.keys().cloned().collect();
            set.remove(&bank.slot());
            ancestors.insert(bank.slot(), set);
        }
        ancestors
    }

    /// Create a map of bank slot id to the set of all of its descendants
    #[allow(clippy::or_fun_call)]
    pub fn descendants(&self) -> HashMap<u64, HashSet<u64>> {
        let mut descendants = HashMap::new();
        for bank in self.banks.values() {
            let _ = descendants.entry(bank.slot()).or_insert(HashSet::new());
            let mut set: HashSet<u64> = bank.ancestors.keys().cloned().collect();
            set.remove(&bank.slot());
            for parent in set {
                descendants
                    .entry(parent)
                    .or_insert(HashSet::new())
                    .insert(bank.slot());
            }
        }
        descendants
    }

    pub fn frozen_banks(&self) -> HashMap<u64, Arc<Bank>> {
        self.banks
            .iter()
            .filter(|(_, b)| b.is_frozen())
            .map(|(k, b)| (*k, b.clone()))
            .collect()
    }

    pub fn active_banks(&self) -> Vec<u64> {
        self.banks
            .iter()
            .filter(|(_, v)| !v.is_frozen())
            .map(|(k, _v)| *k)
            .collect()
    }

    pub fn get(&self, bank_slot: u64) -> Option<&Arc<Bank>> {
        self.banks.get(&bank_slot)
    }

    pub fn new_from_banks(initial_banks: &[Arc<Bank>], root: u64) -> Self {
        let mut banks = HashMap::new();
        let working_bank = initial_banks[0].clone();
        for bank in initial_banks {
            banks.insert(bank.slot(), bank.clone());
        }
        Self {
            root,
            banks,
            working_bank,
            snapshot_config: None,
            last_snapshot: 0,
            confidence: HashMap::new(),
        }
    }

    pub fn insert(&mut self, bank: Bank) -> Arc<Bank> {
        let bank = Arc::new(bank);
        let prev = self.banks.insert(bank.slot(), bank.clone());
        assert!(prev.is_none());

        self.working_bank = bank.clone();
        bank
    }

    // TODO: really want to kill this...
    pub fn working_bank(&self) -> Arc<Bank> {
        self.working_bank.clone()
    }

    pub fn set_root(&mut self, root: u64, snapshot_package_sender: &Option<SnapshotPackageSender>) {
        self.root = root;
        let set_root_start = Instant::now();
        let root_bank = self
            .banks
            .get(&root)
            .expect("root bank didn't exist in bank_forks");
        let root_tx_count = root_bank
            .parents()
            .last()
            .map(|bank| bank.transaction_count())
            .unwrap_or(0);
        root_bank.squash();
        let new_tx_count = root_bank.transaction_count();
        self.prune_non_root(root);

        // Generate a snapshot if snapshots are configured and it's been an appropriate number
        // of banks since the last snapshot
        if self.snapshot_config.is_some() {
            let config = self
                .snapshot_config
                .as_ref()
                .expect("Called package_snapshot without a snapshot configuration");
            if root - self.last_snapshot >= config.snapshot_interval_slots as u64 {
                let mut snapshot_time = Measure::start("total-snapshot-ms");
                let r = self.generate_snapshot(
                    root,
                    snapshot_package_sender.as_ref().unwrap(),
                    snapshot_utils::get_snapshot_tar_path(&config.snapshot_package_output_path),
                );
                if r.is_err() {
                    warn!("Error generating snapshot for bank: {}, err: {:?}", root, r);
                } else {
                    self.last_snapshot = root;
                }

                // Cleanup outdated snapshots
                self.purge_old_snapshots();
                snapshot_time.stop();
                inc_new_counter_info!("total-snapshot-setup-ms", snapshot_time.as_ms() as usize);
            }
        }

        inc_new_counter_info!(
            "bank-forks_set_root_ms",
            timing::duration_as_ms(&set_root_start.elapsed()) as usize
        );
        inc_new_counter_info!(
            "bank-forks_set_root_tx_count",
            (new_tx_count - root_tx_count) as usize
        );
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    fn purge_old_snapshots(&self) {
        // Remove outdated snapshots
        let config = self.snapshot_config.as_ref().unwrap();
        let names = snapshot_utils::get_snapshot_names(&config.snapshot_path);
        let num_to_remove = names.len().saturating_sub(MAX_CACHE_ENTRIES);
        for old_root in &names[..num_to_remove] {
            let r = snapshot_utils::remove_snapshot(*old_root, &config.snapshot_path);
            if r.is_err() {
                warn!("Couldn't remove snapshot at: {:?}", config.snapshot_path);
            }
        }
    }

    fn generate_snapshot<P: AsRef<Path>>(
        &self,
        root: u64,
        snapshot_package_sender: &SnapshotPackageSender,
        tar_output_file: P,
    ) -> Result<()> {
        let config = self.snapshot_config.as_ref().unwrap();

        // Add a snapshot for the new root
        let bank = self
            .get(root)
            .cloned()
            .expect("root must exist in BankForks");
        snapshot_utils::add_snapshot(&config.snapshot_path, &bank, root)?;

        // Package the relevant snapshots
        let names = snapshot_utils::get_snapshot_names(&config.snapshot_path);

        // We only care about the last MAX_CACHE_ENTRIES snapshots of roots because
        // the status cache of anything older is thrown away by the bank in
        // status_cache.prune_roots()
        let start = names.len().saturating_sub(MAX_CACHE_ENTRIES);
        let package = snapshot_utils::package_snapshot(
            &bank,
            &names[start..],
            &config.snapshot_path,
            tar_output_file,
        )?;

        // Send the package to the packaging thread
        snapshot_package_sender.send(package)?;

        Ok(())
    }

    fn prune_non_root(&mut self, root: u64) {
        let descendants = self.descendants();
        self.banks
            .retain(|slot, _| slot == &root || descendants[&root].contains(slot));
        self.confidence
            .retain(|slot, _| slot == &root || descendants[&root].contains(slot));
    }

    pub fn cache_fork_confidence(
        &mut self,
        fork: u64,
        fork_stakes: u64,
        epoch_stakes: u64,
        lockouts: u64,
    ) {
        self.confidence
            .entry(fork)
            .and_modify(|entry| {
                entry.fork_stakes = fork_stakes;
                entry.epoch_stakes = epoch_stakes;
                entry.lockouts = lockouts;
            })
            .or_insert_with(|| Confidence::new(fork_stakes, epoch_stakes, lockouts));
    }

    pub fn cache_stake_weighted_lockouts(&mut self, fork: u64, stake_weighted_lockouts: u128) {
        self.confidence
            .entry(fork)
            .and_modify(|entry| {
                entry.stake_weighted_lockouts = stake_weighted_lockouts;
            })
            .or_insert(Confidence {
                fork_stakes: 0,
                epoch_stakes: 0,
                lockouts: 0,
                stake_weighted_lockouts,
            });
    }

    pub fn get_fork_confidence(&self, fork: u64) -> Option<&Confidence> {
        self.confidence.get(&fork)
    }

    pub fn set_snapshot_config(&mut self, snapshot_config: SnapshotConfig) {
        self.snapshot_config = Some(snapshot_config);
    }

    pub fn snapshot_config(&self) -> &Option<SnapshotConfig> {
        &self.snapshot_config
    }

    fn setup_banks(
        bank_maps: &mut Vec<(u64, u64, Bank)>,
        bank_rc: &BankRc,
        status_cache_rc: &StatusCacheRc,
    ) -> (HashMap<u64, Arc<Bank>>, u64) {
        let mut banks = HashMap::new();
        let (last_slot, last_parent_slot, mut last_bank) = bank_maps.remove(0);
        last_bank.set_bank_rc(&bank_rc, &status_cache_rc);

        while let Some((slot, parent_slot, mut bank)) = bank_maps.pop() {
            bank.set_bank_rc(&bank_rc, &status_cache_rc);
            if parent_slot != 0 {
                if let Some(parent) = banks.get(&parent_slot) {
                    bank.set_parent(parent);
                }
            }
            if slot > 0 {
                banks.insert(slot, Arc::new(bank));
            }
        }
        if last_parent_slot != 0 {
            if let Some(parent) = banks.get(&last_parent_slot) {
                last_bank.set_parent(parent);
            }
        }
        banks.insert(last_slot, Arc::new(last_bank));

        (banks, last_slot)
    }

    pub fn load_from_snapshot(
        genesis_block: &GenesisBlock,
        account_paths: Option<String>,
        snapshot_config: &SnapshotConfig,
    ) -> Result<Self> {
        fs::create_dir_all(&snapshot_config.snapshot_path)?;
        let names = snapshot_utils::get_snapshot_names(&snapshot_config.snapshot_path);
        if names.is_empty() {
            return Err(Error::IO(IOError::new(
                ErrorKind::Other,
                "no snapshots found",
            )));
        }
        let mut bank_maps = vec![];
        let status_cache_rc = StatusCacheRc::default();
        let id = (names[names.len() - 1] + 1) as usize;
        let mut bank0 =
            Bank::create_with_genesis(&genesis_block, account_paths.clone(), &status_cache_rc, id);
        bank0.freeze();
        let bank_root = snapshot_utils::load_snapshots(
            &names,
            &mut bank0,
            &mut bank_maps,
            &status_cache_rc,
            &snapshot_config.snapshot_path,
        );
        if bank_maps.is_empty() || bank_root.is_none() {
            return Err(Error::IO(IOError::new(
                ErrorKind::Other,
                "no snapshots loaded",
            )));
        }

        let root = bank_root.unwrap();
        let (banks, last_slot) =
            BankForks::setup_banks(&mut bank_maps, &bank0.rc, &status_cache_rc);
        let working_bank = banks[&last_slot].clone();

        Ok(BankForks {
            banks,
            working_bank,
            root,
            snapshot_config: None,
            last_snapshot: *names.last().unwrap(),
            confidence: HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use crate::service::Service;
    use crate::snapshot_package::SnapshotPackagerService;
    use fs_extra::dir::CopyOptions;
    use itertools::Itertools;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use tempfile::TempDir;

    #[test]
    fn test_bank_forks() {
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks[0u64], &Pubkey::default(), 1);
        child_bank.register_tick(&Hash::default());
        bank_forks.insert(child_bank);
        assert_eq!(bank_forks[1u64].tick_height(), 1);
        assert_eq!(bank_forks.working_bank().tick_height(), 1);
    }

    #[test]
    fn test_bank_forks_descendants() {
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let bank0 = bank_forks[0].clone();
        let bank = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.insert(bank);
        let bank = Bank::new_from_parent(&bank0, &Pubkey::default(), 2);
        bank_forks.insert(bank);
        let descendants = bank_forks.descendants();
        let children: HashSet<u64> = [1u64, 2u64].to_vec().into_iter().collect();
        assert_eq!(children, *descendants.get(&0).unwrap());
        assert!(descendants[&1].is_empty());
        assert!(descendants[&2].is_empty());
    }

    #[test]
    fn test_bank_forks_ancestors() {
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let bank0 = bank_forks[0].clone();
        let bank = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.insert(bank);
        let bank = Bank::new_from_parent(&bank0, &Pubkey::default(), 2);
        bank_forks.insert(bank);
        let ancestors = bank_forks.ancestors();
        assert!(ancestors[&0].is_empty());
        let parents: Vec<u64> = ancestors[&1].iter().cloned().collect();
        assert_eq!(parents, vec![0]);
        let parents: Vec<u64> = ancestors[&2].iter().cloned().collect();
        assert_eq!(parents, vec![0]);
    }

    #[test]
    fn test_bank_forks_frozen_banks() {
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks[0u64], &Pubkey::default(), 1);
        bank_forks.insert(child_bank);
        assert!(bank_forks.frozen_banks().get(&0).is_some());
        assert!(bank_forks.frozen_banks().get(&1).is_none());
    }

    #[test]
    fn test_bank_forks_active_banks() {
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks[0u64], &Pubkey::default(), 1);
        bank_forks.insert(child_bank);
        assert_eq!(bank_forks.active_banks(), vec![1]);
    }

    #[test]
    fn test_bank_forks_confidence_cache() {
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let fork = bank.slot();
        let mut bank_forks = BankForks::new(0, bank);
        assert!(bank_forks.confidence.get(&fork).is_none());
        bank_forks.cache_fork_confidence(fork, 11, 12, 13);
        assert_eq!(
            bank_forks.confidence.get(&fork).unwrap(),
            &Confidence {
                fork_stakes: 11,
                epoch_stakes: 12,
                lockouts: 13,
                stake_weighted_lockouts: 0,
            }
        );
        // Ensure that {fork_stakes, epoch_stakes, lockouts} and stake_weighted_lockouts
        // can be updated separately
        bank_forks.cache_stake_weighted_lockouts(fork, 20);
        assert_eq!(
            bank_forks.confidence.get(&fork).unwrap(),
            &Confidence {
                fork_stakes: 11,
                epoch_stakes: 12,
                lockouts: 13,
                stake_weighted_lockouts: 20,
            }
        );
        bank_forks.cache_fork_confidence(fork, 21, 22, 23);
        assert_eq!(
            bank_forks
                .confidence
                .get(&fork)
                .unwrap()
                .stake_weighted_lockouts,
            20,
        );
    }

    fn restore_from_snapshot(
        genesis_block: &GenesisBlock,
        bank_forks: BankForks,
        account_paths: Option<String>,
        last_slot: u64,
    ) {
        let snapshot_path = bank_forks
            .snapshot_config
            .as_ref()
            .map(|c| &c.snapshot_path)
            .unwrap();

        let new = BankForks::load_from_snapshot(
            &genesis_block,
            account_paths,
            bank_forks.snapshot_config.as_ref().unwrap(),
        )
        .unwrap();

        for (slot, _) in new.banks.iter() {
            if *slot > 0 {
                let bank = bank_forks.banks.get(slot).unwrap().clone();
                let new_bank = new.banks.get(slot).unwrap();
                bank.compare_bank(&new_bank);
            }
        }

        assert_eq!(new.working_bank().slot(), last_slot);
        for (slot, _) in new.banks.iter() {
            snapshot_utils::remove_snapshot(*slot, snapshot_path).unwrap();
        }
    }

    #[test]
    fn test_bank_forks_snapshot_n() {
        solana_logger::setup();
        let accounts_dir = TempDir::new().unwrap();
        let snapshot_dir = TempDir::new().unwrap();
        let snapshot_output_path = TempDir::new().unwrap();
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10_000);
        for index in 0..10 {
            let bank0 = Bank::new_with_paths(
                &genesis_block,
                Some(accounts_dir.path().to_str().unwrap().to_string()),
            );
            bank0.freeze();
            let mut bank_forks = BankForks::new(0, bank0);
            let snapshot_config = SnapshotConfig::new(
                PathBuf::from(snapshot_dir.path()),
                PathBuf::from(snapshot_output_path.path()),
                100,
            );
            bank_forks.set_snapshot_config(snapshot_config.clone());
            let bank0 = bank_forks.get(0).unwrap();
            snapshot_utils::add_snapshot(&snapshot_config.snapshot_path, bank0, 0).unwrap();
            for forks in 0..index {
                let bank = Bank::new_from_parent(&bank_forks[forks], &Pubkey::default(), forks + 1);
                let key1 = Keypair::new().pubkey();
                let tx = system_transaction::create_user_account(
                    &mint_keypair,
                    &key1,
                    1,
                    genesis_block.hash(),
                );
                assert_eq!(bank.process_transaction(&tx), Ok(()));
                bank.freeze();
                snapshot_utils::add_snapshot(&snapshot_config.snapshot_path, &bank, 0).unwrap();
                bank_forks.insert(bank);
            }
            restore_from_snapshot(
                &genesis_block,
                bank_forks,
                Some(accounts_dir.path().to_str().unwrap().to_string()),
                index,
            );
        }
    }

    #[test]
    fn test_concurrent_snapshot_packaging() {
        solana_logger::setup();
        let accounts_dir = TempDir::new().unwrap();
        let snapshots_dir = TempDir::new().unwrap();
        let snapshot_output_path = TempDir::new().unwrap();
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10_000);
        let (sender, receiver) = channel();
        let (fake_sender, _fake_receiver) = channel();
        let bank0 = Bank::new_with_paths(
            &genesis_block,
            Some(accounts_dir.path().to_str().unwrap().to_string()),
        );
        bank0.freeze();

        // Set up bank forks
        let mut bank_forks = BankForks::new(0, bank0);
        let snapshot_config = SnapshotConfig::new(
            PathBuf::from(snapshots_dir.path()),
            PathBuf::from(snapshot_output_path.path()),
            1,
        );
        bank_forks.set_snapshot_config(snapshot_config.clone());

        // Take snapshot of zeroth bank
        let bank0 = bank_forks.get(0).unwrap();
        snapshot_utils::add_snapshot(&snapshot_config.snapshot_path, bank0, 0).unwrap();

        // Create next MAX_CACHE_ENTRIES + 2 banks and snapshots. Every bank will get snapshotted
        // and the snapshot purging logic will run on every snapshot taken. This means the three
        // (including snapshot for bank0 created above) earliest snapshots will get purged by the
        // time this loop is done.

        // Also, make a saved copy of the state of the snapshot for a bank with
        // bank.slot == saved_slot, so we can use it for a correctness check later.
        let saved_snapshots_dir = TempDir::new().unwrap();
        let saved_accounts_dir = TempDir::new().unwrap();
        let saved_slot = 4;
        let saved_tar = snapshot_config
            .snapshot_package_output_path
            .join(saved_slot.to_string());
        for forks in 0..MAX_CACHE_ENTRIES + 2 {
            let bank = Bank::new_from_parent(
                &bank_forks[forks as u64],
                &Pubkey::default(),
                (forks + 1) as u64,
            );
            let slot = bank.slot();
            let key1 = Keypair::new().pubkey();
            let tx = system_transaction::create_user_account(
                &mint_keypair,
                &key1,
                1,
                genesis_block.hash(),
            );
            assert_eq!(bank.process_transaction(&tx), Ok(()));
            bank.freeze();
            bank_forks.insert(bank);

            let package_sender = {
                if slot == saved_slot as u64 {
                    // Only send one package on the real sende so that the packaging service
                    // doesn't take forever to run the packaging logic on all MAX_CACHE_ENTRIES
                    // later
                    &sender
                } else {
                    &fake_sender
                }
            };

            bank_forks
                .generate_snapshot(
                    slot,
                    &package_sender,
                    snapshot_config
                        .snapshot_package_output_path
                        .join(slot.to_string()),
                )
                .unwrap();

            if slot == saved_slot as u64 {
                let options = CopyOptions::new();
                fs_extra::dir::copy(&accounts_dir, &saved_accounts_dir, &options).unwrap();
                fs_extra::dir::copy(&snapshots_dir, &saved_snapshots_dir, &options).unwrap();
            }
        }

        // Purge all the outdated snapshots, including the ones needed to generate the package
        // currently sitting in the channel
        bank_forks.purge_old_snapshots();
        let mut snapshot_names = snapshot_utils::get_snapshot_names(&snapshots_dir);
        snapshot_names.sort();
        assert_eq!(
            snapshot_names,
            (3..=MAX_CACHE_ENTRIES as u64 + 2).collect_vec()
        );

        // Create a SnapshotPackagerService to create tarballs from all the pending
        // SnapshotPackage's on the channel. By the time this service starts, we have already
        // purged the first two snapshots, which are needed by every snapshot other than
        // the last two snapshots. However, the packaging service should still be able to
        // correctly construct the earlier snapshots because the SnapshotPackage's on the
        // channel hold hard links to these deleted snapshots. We verify this is the case below.
        let exit = Arc::new(AtomicBool::new(false));
        let snapshot_packager_service = SnapshotPackagerService::new(receiver, &exit);

        // Close the channel so that the package service will exit after reading all the
        // packages off the channel
        drop(sender);

        // Wait for service to finish
        snapshot_packager_service
            .join()
            .expect("SnapshotPackagerService exited with error");

        // Check the tar we cached the state for earlier was generated correctly
        snapshot_utils::tests::verify_snapshot_tar(
            saved_tar,
            saved_snapshots_dir
                .path()
                .join(snapshots_dir.path().file_name().unwrap()),
            saved_accounts_dir
                .path()
                .join(accounts_dir.path().file_name().unwrap()),
        );
    }
}
