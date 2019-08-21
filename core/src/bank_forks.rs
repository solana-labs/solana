//! The `bank_forks` module implments BankForks a DAG of checkpointed Banks

use crate::result::Result;
use crate::snapshot_package::SnapshotPackageSender;
use crate::snapshot_utils;
use solana_measure::measure::Measure;
use solana_metrics::inc_new_counter_info;
use solana_runtime::bank::Bank;
use solana_runtime::status_cache::MAX_CACHE_ENTRIES;
use solana_sdk::timing;
use std::collections::{HashMap, HashSet};
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
    slots_since_snapshot: Vec<u64>,
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
            slots_since_snapshot: vec![bank_slot],
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

    pub fn new_from_banks(initial_forks: &[Arc<Bank>], rooted_path: Vec<u64>) -> Self {
        let mut banks = HashMap::new();
        let working_bank = initial_forks[0].clone();

        // Iterate through the heads of all the different forks
        for bank in initial_forks {
            banks.insert(bank.slot(), bank.clone());
            let parents = bank.parents();
            for parent in parents {
                if banks.contains_key(&parent.slot()) {
                    // All ancestors have already been inserted by another fork
                    break;
                }
                banks.insert(parent.slot(), parent.clone());
            }
        }

        Self {
            root: *rooted_path.last().unwrap(),
            banks,
            working_bank,
            snapshot_config: None,
            slots_since_snapshot: rooted_path,
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

        if self.snapshot_config.is_some() && snapshot_package_sender.is_some() {
            let new_rooted_path = root_bank
                .parents()
                .into_iter()
                .map(|p| p.slot())
                .rev()
                .skip(1);
            self.slots_since_snapshot.extend(new_rooted_path);
            self.slots_since_snapshot.push(root);
            if self.slots_since_snapshot.len() > MAX_CACHE_ENTRIES {
                let num_to_remove = self.slots_since_snapshot.len() - MAX_CACHE_ENTRIES;
                self.slots_since_snapshot.drain(0..num_to_remove);
            }
        }

        root_bank.squash();
        let new_tx_count = root_bank.transaction_count();

        // Generate a snapshot if snapshots are configured and it's been an appropriate number
        // of banks since the last snapshot
        if self.snapshot_config.is_some() && snapshot_package_sender.is_some() {
            let config = self
                .snapshot_config
                .as_ref()
                .expect("Called package_snapshot without a snapshot configuration");
            if root - self.slots_since_snapshot[0] >= config.snapshot_interval_slots as u64 {
                let mut snapshot_time = Measure::start("total-snapshot-ms");
                let r = self.generate_snapshot(
                    root,
                    &self.slots_since_snapshot[1..],
                    snapshot_package_sender.as_ref().unwrap(),
                    snapshot_utils::get_snapshot_tar_path(&config.snapshot_package_output_path),
                );
                if r.is_err() {
                    warn!("Error generating snapshot for bank: {}, err: {:?}", root, r);
                } else {
                    self.slots_since_snapshot = vec![root];
                }

                // Cleanup outdated snapshots
                self.purge_old_snapshots();
                snapshot_time.stop();
                inc_new_counter_info!("total-snapshot-setup-ms", snapshot_time.as_ms() as usize);
            }
        }

        self.prune_non_root(root);

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

    pub fn slots_since_snapshot(&self) -> &[u64] {
        &self.slots_since_snapshot
    }

    fn purge_old_snapshots(&self) {
        // Remove outdated snapshots
        let config = self.snapshot_config.as_ref().unwrap();
        let slot_snapshot_paths = snapshot_utils::get_snapshot_paths(&config.snapshot_path);
        let num_to_remove = slot_snapshot_paths.len().saturating_sub(MAX_CACHE_ENTRIES);
        for slot_files in &slot_snapshot_paths[..num_to_remove] {
            let r = snapshot_utils::remove_snapshot(slot_files.slot, &config.snapshot_path);
            if r.is_err() {
                warn!("Couldn't remove snapshot at: {:?}", config.snapshot_path);
            }
        }
    }

    fn generate_snapshot<P: AsRef<Path>>(
        &self,
        root: u64,
        slots_since_snapshot: &[u64],
        snapshot_package_sender: &SnapshotPackageSender,
        tar_output_file: P,
    ) -> Result<()> {
        let config = self.snapshot_config.as_ref().unwrap();

        // Add a snapshot for the new root
        let bank = self
            .get(root)
            .cloned()
            .expect("root must exist in BankForks");
        snapshot_utils::add_snapshot(&config.snapshot_path, &bank, slots_since_snapshot)?;
        // Package the relevant snapshots
        let slot_snapshot_paths = snapshot_utils::get_snapshot_paths(&config.snapshot_path);

        // We only care about the last MAX_CACHE_ENTRIES snapshots of roots because
        // the status cache of anything older is thrown away by the bank in
        // status_cache.prune_roots()
        let start = slot_snapshot_paths.len().saturating_sub(MAX_CACHE_ENTRIES);
        let package = snapshot_utils::package_snapshot(
            &bank,
            &slot_snapshot_paths[start..],
            tar_output_file,
            &config.snapshot_path,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use crate::service::Service;
    use crate::snapshot_package::SnapshotPackagerService;
    use fs_extra::dir::CopyOptions;
    use itertools::Itertools;
    use solana_sdk::hash::{hashv, Hash};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use std::fs;
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

    fn restore_from_snapshot(old_bank_forks: &BankForks, account_paths: String) {
        let (snapshot_path, snapshot_package_output_path) = old_bank_forks
            .snapshot_config
            .as_ref()
            .map(|c| (&c.snapshot_path, &c.snapshot_package_output_path))
            .unwrap();

        let deserialized_bank = snapshot_utils::bank_from_archive(
            account_paths,
            old_bank_forks.snapshot_config.as_ref().unwrap(),
            snapshot_utils::get_snapshot_tar_path(snapshot_package_output_path),
        )
        .unwrap();

        let bank = old_bank_forks
            .banks
            .get(&deserialized_bank.slot())
            .unwrap()
            .clone();
        bank.compare_bank(&deserialized_bank);

        let slot_snapshot_paths = snapshot_utils::get_snapshot_paths(&snapshot_path);

        for p in slot_snapshot_paths {
            snapshot_utils::remove_snapshot(p.slot, &snapshot_path).unwrap();
        }
    }

    // creates banks up to "last_slot" and runs the input function `f` on each bank created
    // also marks each bank as root and generates snapshots
    // finally tries to restore from the last bank's snapshot and compares the restored bank to the
    // `last_slot` bank
    fn run_bank_forks_snapshot_n<F>(last_slot: u64, f: F, set_root_interval: u64)
    where
        F: Fn(&mut Bank, &Keypair),
    {
        solana_logger::setup();
        // Set up snapshotting config
        let mut snapshot_test_config = setup_snapshot_test(1);

        let bank_forks = &mut snapshot_test_config.bank_forks;
        let accounts_dir = &snapshot_test_config.accounts_dir;
        let snapshot_config = &snapshot_test_config.snapshot_config;
        let mint_keypair = &snapshot_test_config.genesis_block_info.mint_keypair;

        let (s, _r) = channel();
        let sender = Some(s);
        for slot in 0..last_slot {
            let mut bank = Bank::new_from_parent(&bank_forks[slot], &Pubkey::default(), slot + 1);
            f(&mut bank, &mint_keypair);
            let bank = bank_forks.insert(bank);
            // Set root to make sure we don't end up with too many account storage entries
            // and to allow snapshotting of bank and the purging logic on status_cache to
            // kick in
            if slot % set_root_interval == 0 || slot == last_slot - 1 {
                bank_forks.set_root(bank.slot(), &sender);
            }
        }
        // Generate a snapshot package for last bank
        let last_bank = bank_forks.get(last_slot).unwrap();
        let slot_snapshot_paths =
            snapshot_utils::get_snapshot_paths(&snapshot_config.snapshot_path);
        let snapshot_package = snapshot_utils::package_snapshot(
            last_bank,
            &slot_snapshot_paths,
            snapshot_utils::get_snapshot_tar_path(&snapshot_config.snapshot_package_output_path),
            &snapshot_config.snapshot_path,
        )
        .unwrap();
        SnapshotPackagerService::package_snapshots(&snapshot_package).unwrap();

        restore_from_snapshot(
            bank_forks,
            accounts_dir.path().to_str().unwrap().to_string(),
        );
    }

    #[test]
    fn test_bank_forks_snapshot_n() {
        // create banks upto slot 4 and create 1 new account in each bank. test that bank 4 snapshots
        // and restores correctly
        run_bank_forks_snapshot_n(
            4,
            |bank, mint_keypair| {
                let key1 = Keypair::new().pubkey();
                let tx = system_transaction::create_user_account(
                    &mint_keypair,
                    &key1,
                    1,
                    bank.last_blockhash(),
                );
                assert_eq!(bank.process_transaction(&tx), Ok(()));
                bank.freeze();
            },
            1,
        );
    }

    fn goto_end_of_slot(bank: &mut Bank) {
        let mut tick_hash = bank.last_blockhash();
        loop {
            tick_hash = hashv(&[&tick_hash.as_ref(), &[42]]);
            bank.register_tick(&tick_hash);
            if tick_hash == bank.last_blockhash() {
                bank.freeze();
                return;
            }
        }
    }

    #[test]
    fn test_bank_forks_status_cache_snapshot_n() {
        // create banks upto slot (MAX_CACHE_ENTRIES * 2) + 1 while transferring 1 lamport into 2 different accounts each time
        // this is done to ensure the AccountStorageEntries keep getting cleaned up as the root moves
        // ahead. Also tests the status_cache purge and status cache snapshotting.
        // Makes sure that the last bank is restored correctly
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        for set_root_interval in &[1, 4] {
            run_bank_forks_snapshot_n(
                (MAX_CACHE_ENTRIES * 2 + 1) as u64,
                |bank, mint_keypair| {
                    let tx = system_transaction::transfer(
                        &mint_keypair,
                        &key1,
                        1,
                        bank.parent().unwrap().last_blockhash(),
                    );
                    assert_eq!(bank.process_transaction(&tx), Ok(()));
                    let tx = system_transaction::transfer(
                        &mint_keypair,
                        &key2,
                        1,
                        bank.parent().unwrap().last_blockhash(),
                    );
                    assert_eq!(bank.process_transaction(&tx), Ok(()));
                    goto_end_of_slot(bank);
                },
                *set_root_interval,
            );
        }
    }

    #[test]
    fn test_concurrent_snapshot_packaging() {
        solana_logger::setup();

        // Set up snapshotting config
        let mut snapshot_test_config = setup_snapshot_test(1);

        let bank_forks = &mut snapshot_test_config.bank_forks;
        let accounts_dir = &snapshot_test_config.accounts_dir;
        let snapshots_dir = &snapshot_test_config.snapshot_dir;
        let snapshot_config = &snapshot_test_config.snapshot_config;
        let mint_keypair = &snapshot_test_config.genesis_block_info.mint_keypair;
        let genesis_block = &snapshot_test_config.genesis_block_info.genesis_block;

        // Take snapshot of zeroth bank
        let bank0 = bank_forks.get(0).unwrap();
        snapshot_utils::add_snapshot(&snapshot_config.snapshot_path, bank0, &vec![]).unwrap();

        // Set up snapshotting channels
        let (sender, receiver) = channel();
        let (fake_sender, _fake_receiver) = channel();

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
                    // Only send one package on the real sender so that the packaging service
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
                    &vec![],
                    &package_sender,
                    snapshot_config
                        .snapshot_package_output_path
                        .join(slot.to_string()),
                )
                .unwrap();

            if slot == saved_slot as u64 {
                let options = CopyOptions::new();
                fs_extra::dir::copy(accounts_dir, &saved_accounts_dir, &options).unwrap();
                let snapshot_paths: Vec<_> = fs::read_dir(&snapshot_config.snapshot_path)
                    .unwrap()
                    .filter_map(|entry| {
                        let e = entry.unwrap();
                        let file_path = e.path();
                        let file_name = file_path.file_name().unwrap();
                        file_name
                            .to_str()
                            .map(|s| s.parse::<u64>().ok().map(|_| file_path.clone()))
                            .unwrap_or(None)
                    })
                    .collect();

                for snapshot_path in snapshot_paths {
                    fs_extra::dir::copy(&snapshot_path, &saved_snapshots_dir, &options).unwrap();
                }
            }
        }

        // Purge all the outdated snapshots, including the ones needed to generate the package
        // currently sitting in the channel
        bank_forks.purge_old_snapshots();
        let mut snapshot_paths = snapshot_utils::get_snapshot_paths(&snapshots_dir);
        snapshot_paths.sort();
        assert_eq!(
            snapshot_paths.iter().map(|path| path.slot).collect_vec(),
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
            saved_snapshots_dir.path(),
            saved_accounts_dir
                .path()
                .join(accounts_dir.path().file_name().unwrap()),
        );
    }

    #[test]
    #[ignore]
    fn test_slots_since_snapshot() {
        solana_logger::setup();
        for add_root_interval in 1..10 {
            let (snapshot_sender, _snapshot_receiver) = channel();
            let num_set_roots = MAX_CACHE_ENTRIES * 5;
            // Make sure this test never clears bank.slots_since_snapshot
            let mut snapshot_test_config =
                setup_snapshot_test(add_root_interval * num_set_roots * 2);
            let mut current_bank = snapshot_test_config.bank_forks[0].clone();
            let snapshot_sender = Some(snapshot_sender);
            for _ in 0..num_set_roots {
                for _ in 0..add_root_interval {
                    let new_slot = current_bank.slot() + 1;
                    let new_bank =
                        Bank::new_from_parent(&current_bank, &Pubkey::default(), new_slot);
                    snapshot_test_config.bank_forks.insert(new_bank);
                    current_bank = snapshot_test_config.bank_forks[new_slot].clone();
                }
                snapshot_test_config
                    .bank_forks
                    .set_root(current_bank.slot(), &snapshot_sender);

                let slots_since_snapshot_hashset: HashSet<_> = snapshot_test_config
                    .bank_forks
                    .slots_since_snapshot
                    .iter()
                    .cloned()
                    .collect();
                assert_eq!(slots_since_snapshot_hashset, current_bank.src.roots());
            }

            let expected_slots_since_snapshot =
                (0..=num_set_roots as u64 * add_root_interval as u64).collect_vec();
            let num_old_slots = expected_slots_since_snapshot.len() - MAX_CACHE_ENTRIES;

            assert_eq!(
                snapshot_test_config.bank_forks.slots_since_snapshot(),
                &expected_slots_since_snapshot[num_old_slots..],
            );
        }
    }

    struct SnapshotTestConfig {
        accounts_dir: TempDir,
        snapshot_dir: TempDir,
        _snapshot_output_path: TempDir,
        snapshot_config: SnapshotConfig,
        bank_forks: BankForks,
        genesis_block_info: GenesisBlockInfo,
    }

    fn setup_snapshot_test(snapshot_interval: usize) -> SnapshotTestConfig {
        let accounts_dir = TempDir::new().unwrap();
        let snapshot_dir = TempDir::new().unwrap();
        let snapshot_output_path = TempDir::new().unwrap();
        let genesis_block_info = create_genesis_block(10_000);
        let bank0 = Bank::new_with_paths(
            &genesis_block_info.genesis_block,
            Some(accounts_dir.path().to_str().unwrap().to_string()),
        );
        bank0.freeze();
        let mut bank_forks = BankForks::new(0, bank0);

        let snapshot_config = SnapshotConfig::new(
            PathBuf::from(snapshot_dir.path()),
            PathBuf::from(snapshot_output_path.path()),
            snapshot_interval,
        );
        bank_forks.set_snapshot_config(snapshot_config.clone());
        SnapshotTestConfig {
            accounts_dir,
            snapshot_dir,
            _snapshot_output_path: snapshot_output_path,
            snapshot_config,
            bank_forks,
            genesis_block_info,
        }
    }
}
