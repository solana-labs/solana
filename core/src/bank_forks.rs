//! The `bank_forks` module implments BankForks a DAG of checkpointed Banks

use bincode::{deserialize_from, serialize_into};
use solana_metrics::counter::Counter;
use solana_runtime::bank::{Bank, BankRc};
use solana_sdk::timing;
use std::collections::{HashMap, HashSet};
use std::env;
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
    use_snapshot: bool,
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
            use_snapshot: false,
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
            use_snapshot: false,
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
        root_bank.squash();
        self.prune_non_root(root);

        inc_new_counter_info!(
            "bank-forks_set_root_ms",
            timing::duration_as_ms(&set_root_start.elapsed()) as usize
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
        if self.use_snapshot {
            let diff: HashSet<_> = slots.symmetric_difference(&self.slots).collect();
            trace!("prune non root {} - {:?}", root, diff);
            for slot in diff.iter() {
                if **slot > root {
                    let _ = self.add_snapshot(**slot, root);
                } else if **slot > 0 {
                    self.remove_snapshot(**slot);
                }
            }
        }
        self.slots = slots.clone();
    }

    fn get_io_error(error: &str) -> Error {
        Error::new(ErrorKind::Other, error)
    }

    fn get_snapshot_path() -> PathBuf {
        let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let snapshot_dir = format!("{}/snapshots/", out_dir);
        Path::new(&snapshot_dir).to_path_buf()
    }

    pub fn add_snapshot(&self, slot: u64, root: u64) -> Result<(), Error> {
        let path = BankForks::get_snapshot_path();
        fs::create_dir_all(path.clone())?;
        let bank_file = format!("{}", slot);
        let bank_file_path = path.join(bank_file);
        trace!("path: {:?}", bank_file_path);
        let file = File::create(bank_file_path)?;
        let mut stream = BufWriter::new(file);
        let bank = self.get(slot).unwrap().clone();
        serialize_into(&mut stream, &*bank)
            .map_err(|_| BankForks::get_io_error("serialize bank error"))?;
        let mut parent_slot: u64 = 0;
        if let Some(parent_bank) = bank.parent() {
            parent_slot = parent_bank.slot();
        }
        serialize_into(&mut stream, &parent_slot)
            .map_err(|_| BankForks::get_io_error("serialize bank parent error"))?;
        serialize_into(&mut stream, &bank.rc)
            .map_err(|_| BankForks::get_io_error("serialize bank rc error"))?;
        serialize_into(&mut stream, &root)
            .map_err(|_| BankForks::get_io_error("serialize root error"))?;
        Ok(())
    }

    pub fn remove_snapshot(&self, slot: u64) {
        let path = BankForks::get_snapshot_path();
        let bank_file = format!("{}", slot);
        let bank_file_path = path.join(bank_file);
        let _ = fs::remove_file(bank_file_path);
    }

    pub fn set_snapshot_config(&mut self, use_snapshot: bool) {
        self.use_snapshot = use_snapshot;
    }

    fn setup_banks(
        bank_maps: &mut Vec<(u64, u64, Bank)>,
        bank_rc: &BankRc,
    ) -> (HashMap<u64, Arc<Bank>>, HashSet<u64>, u64) {
        let mut banks = HashMap::new();
        let mut slots = HashSet::new();
        let (last_slot, last_parent_slot, mut last_bank) = bank_maps.remove(0);
        last_bank.set_bank_rc(&bank_rc);

        while let Some((slot, parent_slot, mut bank)) = bank_maps.pop() {
            bank.set_bank_rc(&bank_rc);
            if parent_slot != 0 {
                if let Some(parent) = banks.get(&parent_slot) {
                    bank.set_parent(parent);
                }
            }
            banks.insert(slot, Arc::new(bank));
            slots.insert(slot);
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

    pub fn load_from_snapshot() -> Result<Self, Error> {
        let path = BankForks::get_snapshot_path();
        let paths = fs::read_dir(path.clone())?;
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
        let mut bank_rc: Option<BankRc> = None;
        let mut root: u64 = 0;
        for bank_slot in names.iter().rev() {
            let bank_path = format!("{}", bank_slot);
            let bank_file_path = path.join(bank_path.clone());
            info!("Load from {:?}", bank_file_path);
            let file = File::open(bank_file_path)?;
            let mut stream = BufReader::new(file);
            let bank: Result<Bank, std::io::Error> = deserialize_from(&mut stream)
                .map_err(|_| BankForks::get_io_error("deserialize bank error"));
            let slot: Result<u64, std::io::Error> = deserialize_from(&mut stream)
                .map_err(|_| BankForks::get_io_error("deserialize bank parent error"));
            let parent_slot = if slot.is_ok() { slot.unwrap() } else { 0 };
            if bank_rc.is_none() {
                let rc: Result<BankRc, std::io::Error> = deserialize_from(&mut stream)
                    .map_err(|_| BankForks::get_io_error("deserialize bank rc error"));
                if rc.is_ok() {
                    bank_rc = Some(rc.unwrap());
                    let r: Result<u64, std::io::Error> = deserialize_from(&mut stream)
                        .map_err(|_| BankForks::get_io_error("deserialize root error"));
                    if r.is_ok() {
                        root = r.unwrap();
                    }
                }
            }
            match bank {
                Ok(v) => bank_maps.push((*bank_slot, parent_slot, v)),
                Err(_) => warn!("Load snapshot failed for {}", bank_slot),
            }
        }
        if bank_maps.is_empty() || bank_rc.is_none() {
            return Err(Error::new(ErrorKind::Other, "no snapshots loaded"));
        }

        let (banks, slots, last_slot) = BankForks::setup_banks(&mut bank_maps, &bank_rc.unwrap());
        let working_bank = banks[&last_slot].clone();
        Ok(BankForks {
            banks,
            working_bank,
            root,
            slots,
            use_snapshot: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::create_genesis_block;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use std::env;
    use std::fs::remove_dir_all;

    #[test]
    fn test_bank_forks() {
        let (genesis_block, _) = create_genesis_block(10_000);
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
        let (genesis_block, _) = create_genesis_block(10_000);
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
        let (genesis_block, _) = create_genesis_block(10_000);
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
        let (genesis_block, _) = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks[0u64], &Pubkey::default(), 1);
        bank_forks.insert(child_bank);
        assert!(bank_forks.frozen_banks().get(&0).is_some());
        assert!(bank_forks.frozen_banks().get(&1).is_none());
    }

    #[test]
    fn test_bank_forks_active_banks() {
        let (genesis_block, _) = create_genesis_block(10_000);
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

    fn save_and_load_snapshot(bank_forks: &BankForks) {
        for (slot, _) in bank_forks.banks.iter() {
            bank_forks.add_snapshot(*slot, 0).unwrap();
        }

        let new = BankForks::load_from_snapshot().unwrap();
        for (slot, _) in bank_forks.banks.iter() {
            let bank = bank_forks.banks.get(slot).unwrap().clone();
            let new_bank = new.banks.get(slot).unwrap();
            bank.compare_bank(&new_bank);
        }
        for (slot, _) in new.banks.iter() {
            new.remove_snapshot(*slot);
        }
    }

    #[test]
    fn test_bank_forks_snapshot_n() {
        solana_logger::setup();
        let path = get_tmp_bank_accounts_path!();
        let (genesis_block, mint_keypair) = create_genesis_block(10_000);
        let bank0 = Bank::new_with_paths(&genesis_block, Some(path.paths.clone()));
        bank0.freeze();
        let mut bank_forks = BankForks::new(0, bank0);
        bank_forks.set_snapshot_config(true);
        for index in 0..10 {
            let bank = Bank::new_from_parent(&bank_forks[index], &Pubkey::default(), index + 1);
            let key1 = Keypair::new().pubkey();
            let tx = system_transaction::create_user_account(
                &mint_keypair,
                &key1,
                1,
                genesis_block.hash(),
                0,
            );
            assert_eq!(bank.process_transaction(&tx), Ok(()));
            bank.freeze();
            bank_forks.insert(bank);
            save_and_load_snapshot(&bank_forks);
        }
        assert_eq!(bank_forks.working_bank().slot(), 10);
    }
}
