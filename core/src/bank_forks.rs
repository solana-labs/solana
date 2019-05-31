//! The `bank_forks` module implments BankForks a DAG of checkpointed Banks

use bincode::{deserialize_from, serialize_into};
use solana_metrics::inc_new_counter_info;
use solana_runtime::bank::{Bank, BankRc, StatusCacheRc};
use solana_sdk::timing;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Error, ErrorKind};
use std::ops::Index;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub struct BankForks {
    banks: HashMap<u64, Arc<Bank>>,
    working_bank: Arc<Bank>,
    root: u64,
    slots: HashSet<u64>,
    snapshot_path: Option<String>,
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
            slots: HashSet::new(),
            snapshot_path: None,
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
            slots: HashSet::new(),
            snapshot_path: None,
        }
    }

    pub fn insert(&mut self, bank: Bank) {
        let bank = Arc::new(bank);
        let prev = self.banks.insert(bank.slot(), bank.clone());
        assert!(prev.is_none());

        self.working_bank = bank.clone();
    }

    // TODO: really want to kill this...
    pub fn working_bank(&self) -> Arc<Bank> {
        self.working_bank.clone()
    }

    pub fn set_root(&mut self, root: u64) {
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

    fn prune_non_root(&mut self, root: u64) {
        let slots: HashSet<u64> = self
            .banks
            .iter()
            .filter(|(_, b)| b.is_frozen())
            .map(|(k, _)| *k)
            .collect();
        let descendants = self.descendants();
        self.banks
            .retain(|slot, _| descendants[&root].contains(slot));
        if self.snapshot_path.is_some() {
            let diff: HashSet<_> = slots.symmetric_difference(&self.slots).collect();
            trace!("prune non root {} - {:?}", root, diff);
            for slot in diff.iter() {
                if **slot > root {
                    let _ = self.add_snapshot(**slot, root);
                } else if **slot > 0 {
                    BankForks::remove_snapshot(**slot, &self.snapshot_path);
                }
            }
        }
        self.slots = slots.clone();
    }

    fn get_io_error(error: &str) -> Error {
        warn!("BankForks error: {:?}", error);
        Error::new(ErrorKind::Other, error)
    }

    fn get_snapshot_path(path: &Option<String>) -> PathBuf {
        Path::new(&path.clone().unwrap()).to_path_buf()
    }

    pub fn add_snapshot(&self, slot: u64, root: u64) -> Result<(), Error> {
        let path = BankForks::get_snapshot_path(&self.snapshot_path);
        fs::create_dir_all(path.clone())?;
        let bank_file = format!("{}", slot);
        let bank_file_path = path.join(bank_file);
        trace!("path: {:?}", bank_file_path);
        let file = File::create(bank_file_path)?;
        let mut stream = BufWriter::new(file);
        let bank_slot = self.get(slot);
        if bank_slot.is_none() {
            return Err(BankForks::get_io_error("bank_forks get error"));
        }
        let bank = bank_slot.unwrap().clone();
        serialize_into(&mut stream, &*bank)
            .map_err(|_| BankForks::get_io_error("serialize bank error"))?;
        let mut parent_slot: u64 = 0;
        if let Some(parent_bank) = bank.parent() {
            parent_slot = parent_bank.slot();
        }
        serialize_into(&mut stream, &parent_slot)
            .map_err(|_| BankForks::get_io_error("serialize bank parent error"))?;
        serialize_into(&mut stream, &root)
            .map_err(|_| BankForks::get_io_error("serialize root error"))?;
        serialize_into(&mut stream, &bank.src)
            .map_err(|_| BankForks::get_io_error("serialize bank status cache error"))?;
        serialize_into(&mut stream, &bank.rc)
            .map_err(|_| BankForks::get_io_error("serialize bank accounts error"))?;
        Ok(())
    }

    pub fn remove_snapshot(slot: u64, path: &Option<String>) {
        let path = BankForks::get_snapshot_path(path);
        let bank_file = format!("{}", slot);
        let bank_file_path = path.join(bank_file);
        let _ = fs::remove_file(bank_file_path);
    }

    pub fn set_snapshot_config(&mut self, path: Option<String>) {
        self.snapshot_path = path;
    }

    fn load_snapshots(
        names: &[u64],
        bank_maps: &mut Vec<(u64, u64, Bank)>,
        status_cache_rc: &StatusCacheRc,
        snapshot_path: &Option<String>,
    ) -> Option<(BankRc, u64)> {
        let path = BankForks::get_snapshot_path(snapshot_path);
        let mut bank_rc: Option<(BankRc, u64)> = None;

        for bank_slot in names.iter().rev() {
            let bank_path = format!("{}", bank_slot);
            let bank_file_path = path.join(bank_path.clone());
            info!("Load from {:?}", bank_file_path);
            let file = File::open(bank_file_path);
            if file.is_err() {
                warn!("Snapshot file open failed for {}", bank_slot);
                continue;
            }
            let file = file.unwrap();
            let mut stream = BufReader::new(file);
            let bank: Result<Bank, std::io::Error> = deserialize_from(&mut stream)
                .map_err(|_| BankForks::get_io_error("deserialize bank error"));
            let slot: Result<u64, std::io::Error> = deserialize_from(&mut stream)
                .map_err(|_| BankForks::get_io_error("deserialize bank parent error"));
            let parent_slot = if slot.is_ok() { slot.unwrap() } else { 0 };
            let root: Result<u64, std::io::Error> = deserialize_from(&mut stream)
                .map_err(|_| BankForks::get_io_error("deserialize root error"));
            let status_cache: Result<StatusCacheRc, std::io::Error> = deserialize_from(&mut stream)
                .map_err(|_| BankForks::get_io_error("deserialize bank status cache error"));
            if bank_rc.is_none() {
                let rc: Result<BankRc, std::io::Error> = deserialize_from(&mut stream)
                    .map_err(|_| BankForks::get_io_error("deserialize bank accounts error"));
                if rc.is_ok() {
                    bank_rc = Some((rc.unwrap(), root.unwrap()));
                }
            }
            if bank_rc.is_some() {
                match bank {
                    Ok(v) => {
                        if status_cache.is_ok() {
                            status_cache_rc.append(&status_cache.unwrap());
                        }
                        bank_maps.push((*bank_slot, parent_slot, v));
                    }
                    Err(_) => warn!("Load snapshot failed for {}", bank_slot),
                }
            } else {
                BankForks::remove_snapshot(*bank_slot, snapshot_path);
                warn!("Load snapshot rc failed for {}", bank_slot);
            }
        }
        bank_rc
    }

    fn setup_banks(
        bank_maps: &mut Vec<(u64, u64, Bank)>,
        bank_rc: &BankRc,
        status_cache_rc: &StatusCacheRc,
    ) -> (HashMap<u64, Arc<Bank>>, HashSet<u64>, u64) {
        let mut banks = HashMap::new();
        let mut slots = HashSet::new();
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
                slots.insert(slot);
            }
        }
        if last_parent_slot != 0 {
            if let Some(parent) = banks.get(&last_parent_slot) {
                last_bank.set_parent(parent);
            }
        }
        banks.insert(last_slot, Arc::new(last_bank));
        slots.insert(last_slot);

        (banks, slots, last_slot)
    }

    pub fn load_from_snapshot(snapshot_path: &Option<String>) -> Result<Self, Error> {
        let path = BankForks::get_snapshot_path(snapshot_path);
        let paths = fs::read_dir(path)?;
        let mut names = paths
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    e.path()
                        .file_name()
                        .and_then(|n| n.to_str().map(|s| s.parse::<u64>().unwrap()))
                })
            })
            .collect::<Vec<u64>>();

        names.sort();
        let mut bank_maps = vec![];
        let status_cache_rc = StatusCacheRc::default();
        let rc = BankForks::load_snapshots(&names, &mut bank_maps, &status_cache_rc, snapshot_path);
        if bank_maps.is_empty() || rc.is_none() {
            BankForks::remove_snapshot(0, snapshot_path);
            return Err(Error::new(ErrorKind::Other, "no snapshots loaded"));
        }

        let (bank_rc, root) = rc.unwrap();
        let (banks, slots, last_slot) =
            BankForks::setup_banks(&mut bank_maps, &bank_rc, &status_cache_rc);
        let working_bank = banks[&last_slot].clone();
        Ok(BankForks {
            banks,
            working_bank,
            root,
            slots,
            snapshot_path: snapshot_path.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use std::env;
    use std::fs::remove_dir_all;

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

    struct TempPaths {
        pub paths: String,
    }

    #[macro_export]
    macro_rules! tmp_bank_accounts_name {
        () => {
            &format!("{}-{}", file!(), line!())
        };
    }

    #[macro_export]
    macro_rules! get_tmp_bank_accounts_path {
        () => {
            get_tmp_bank_accounts_path(tmp_bank_accounts_name!())
        };
    }

    impl Drop for TempPaths {
        fn drop(&mut self) {
            let paths: Vec<String> = self.paths.split(',').map(|s| s.to_string()).collect();
            paths.iter().for_each(|p| {
                let _ignored = remove_dir_all(p);
            });
        }
    }

    fn get_paths_vec(paths: &str) -> Vec<String> {
        paths.split(',').map(|s| s.to_string()).collect()
    }

    fn get_tmp_snapshots_path() -> TempPaths {
        let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let path = format!("{}/snapshots", out_dir);
        TempPaths {
            paths: path.to_string(),
        }
    }

    fn get_tmp_bank_accounts_path(paths: &str) -> TempPaths {
        let vpaths = get_paths_vec(paths);
        let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let vpaths: Vec<_> = vpaths
            .iter()
            .map(|path| format!("{}/{}", out_dir, path))
            .collect();
        TempPaths {
            paths: vpaths.join(","),
        }
    }

    fn restore_from_snapshot(bank_forks: BankForks, last_slot: u64) {
        let new = BankForks::load_from_snapshot(&bank_forks.snapshot_path).unwrap();
        for (slot, _) in new.banks.iter() {
            let bank = bank_forks.banks.get(slot).unwrap().clone();
            let new_bank = new.banks.get(slot).unwrap();
            bank.compare_bank(&new_bank);
        }
        assert_eq!(new.working_bank().slot(), last_slot);
        for (slot, _) in new.banks.iter() {
            BankForks::remove_snapshot(*slot, &bank_forks.snapshot_path);
        }
    }

    #[test]
    fn test_bank_forks_snapshot_n() {
        solana_logger::setup();
        let path = get_tmp_bank_accounts_path!();
        let spath = get_tmp_snapshots_path();
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10_000);
        for index in 0..10 {
            let bank0 = Bank::new_with_paths(&genesis_block, Some(path.paths.clone()));
            bank0.freeze();
            let slot = bank0.slot();
            let mut bank_forks = BankForks::new(0, bank0);
            bank_forks.set_snapshot_config(Some(spath.paths.clone()));
            bank_forks.add_snapshot(slot, 0).unwrap();
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
                let slot = bank.slot();
                bank_forks.insert(bank);
                bank_forks.add_snapshot(slot, 0).unwrap();
            }
            restore_from_snapshot(bank_forks, index);
        }
    }
}
