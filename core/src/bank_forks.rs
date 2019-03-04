//! The `bank_forks` module implments BankForks a DAG of checkpointed Banks

use solana_runtime::bank::Bank;
use std::collections::HashMap;
use std::ops::Index;
use std::sync::Arc;

use solana_runtime::accounts::{deserialize_object, serialize_object, Accounts};
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

pub struct BankForks {
    banks: HashMap<u64, Arc<Bank>>,
    working_bank: Arc<Bank>,
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
        }
    }
    pub fn frozen_banks(&self) -> HashMap<u64, Arc<Bank>> {
        let mut frozen_banks: Vec<Arc<Bank>> = vec![];
        frozen_banks.extend(self.banks.values().filter(|v| v.is_frozen()).cloned());
        frozen_banks.extend(
            self.banks
                .iter()
                .flat_map(|(_, v)| v.parents())
                .filter(|v| v.is_frozen()),
        );
        frozen_banks.into_iter().map(|b| (b.slot(), b)).collect()
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

    pub fn new_from_banks(initial_banks: &[Arc<Bank>]) -> Self {
        let mut banks = HashMap::new();
        let working_bank = initial_banks[0].clone();
        for bank in initial_banks {
            banks.insert(bank.slot(), bank.clone());
        }
        Self {
            banks,
            working_bank,
        }
    }

    // TODO: use the bank's own ID instead of receiving a parameter?
    pub fn insert(&mut self, bank_slot: u64, bank: Bank) {
        let mut bank = Arc::new(bank);
        assert_eq!(bank_slot, bank.slot());
        let prev = self.banks.insert(bank_slot, bank.clone());
        assert!(prev.is_none());

        if bank_slot > self.working_bank.slot() {
            self.working_bank = bank.clone()
        }

        // TODO: this really only needs to look at the first
        //  parent if we're always calling insert()
        //  when we construct a child bank
        while let Some(parent) = bank.parent() {
            if let Some(prev) = self.banks.remove(&parent.slot()) {
                assert!(Arc::ptr_eq(&prev, &parent));
            }
            bank = parent;
        }
    }

    // TODO: really want to kill this...
    pub fn working_bank(&self) -> Arc<Bank> {
        self.working_bank.clone()
    }

    pub fn save_snapshot(&self, path: &str) -> Result<(), ()> {
        let mut bank_index: HashMap<u64, PathBuf> = HashMap::new();
        let path = Path::new(&path);
        fs::create_dir_all(path).unwrap();
        for (slot, bank) in &self.frozen_banks() {
            let bank_file = format!("bank-{}.snapshot", slot);
            let bank_file_path = path.join(bank_file);
            bank_index.insert(*slot, bank_file_path.clone());
            let file = File::create(bank_file_path).unwrap();
            let mut stream = BufWriter::new(file);
            bank.serialize_into(&mut stream).unwrap();
        }
        let temp_path = path.join("bank.index.tmp");
        {
            let file = File::create(temp_path.clone()).unwrap();
            let mut stream = BufWriter::new(file);
            serialize_object(&bank_index, &mut stream).unwrap();
            let banks = self.frozen_banks();
            if !banks.is_empty() {
                banks
                    .values()
                    .next()
                    .unwrap()
                    .accounts()
                    .serialize_into(&mut stream)
                    .unwrap();
            }
        }
        fs::rename(temp_path, path.join("bank.index")).unwrap();
        Ok(())
    }

    pub fn load_from_snapshot(path: &str) -> Result<Self, ()> {
        let (bank_index, accounts) = {
            let path = Path::new(path).join("bank.index");
            let index_file = File::open(path).unwrap();
            let mut stream = BufReader::new(index_file);
            let bank_index: HashMap<u64, PathBuf> = deserialize_object(&mut stream).unwrap();
            assert!(!bank_index.is_empty());
            let accounts = Arc::new(Accounts::deserialize_from(&mut stream).unwrap());
            (bank_index, accounts)
        };

        let mut banks: HashMap<u64, Arc<Bank>> = HashMap::new();
        for (slot, bank_path) in &bank_index {
            let file = File::open(bank_path).unwrap();
            let mut stream = BufReader::new(file);
            let mut bank = Bank::deserialize_from(&mut stream).unwrap();
            bank.set_accounts(accounts.clone());
            banks.insert(*slot, Arc::new(bank));
        }
        let working_bank = banks[&0].clone();
        Ok(BankForks {
            banks,
            working_bank,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_bank_forks() {
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks[0u64], Pubkey::default(), 1);
        child_bank.register_tick(&Hash::default());
        bank_forks.insert(1, child_bank);
        assert_eq!(bank_forks[1u64].tick_height(), 1);
        assert_eq!(bank_forks.working_bank().tick_height(), 1);
    }

    #[test]
    fn test_bank_forks_frozen_banks() {
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks[0u64], Pubkey::default(), 1);
        bank_forks.insert(1, child_bank);
        assert!(bank_forks.frozen_banks().get(&0).is_some());
        assert!(bank_forks.frozen_banks().get(&1).is_none());
    }

    #[test]
    fn test_bank_forks_active_banks() {
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks[0u64], Pubkey::default(), 1);
        bank_forks.insert(1, child_bank);
        assert_eq!(bank_forks.active_banks(), vec![1]);
    }

    #[test]
    fn test_bank_forks_snapshot() {
        solana_logger::setup();
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        bank.freeze();
        let tick_height = bank.tick_height();
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks[0u64], Pubkey::default(), 1);
        child_bank.freeze();
        bank_forks.insert(1, child_bank);
        let snapshot_path = "snapshots";
        bank_forks.save_snapshot(snapshot_path).unwrap();
        drop(bank_forks);
        let new = BankForks::load_from_snapshot(snapshot_path).unwrap();
        assert_eq!(new[0].tick_height(), tick_height);
        let _ = fs::remove_dir_all(snapshot_path);
    }

}
