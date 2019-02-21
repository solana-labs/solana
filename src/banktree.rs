//! The `banktree` module implments Banktree a DAG of checkpointed Banks

use solana_runtime::bank::Bank;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Banktree {
    working_bank_id: u64,
    banks: HashMap<u64, Arc<Bank>>,
}

impl Banktree {
    pub fn new(working_bank_id: u64, bank: Bank) -> Self {
        let mut banks = HashMap::new();
        banks.insert(working_bank_id, Arc::new(bank));
        Self {
            working_bank_id,
            banks,
        }
    }

    pub fn working_bank(&self) -> Arc<Bank> {
        self.banks[&self.working_bank_id].clone()
    }

    pub fn insert(&mut self, bank_id: u64, bank: Bank) {
        self.banks.insert(bank_id, Arc::new(bank));
    }

    pub fn set_working_bank_id(&mut self, bank_id: u64) {
        if self.banks.contains_key(&bank_id) {
            self.working_bank_id = bank_id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::Hash;

    #[test]
    fn test_banktree_root() {
        let bank = Bank::default();
        let tick_height = bank.tick_height();
        let banktree = Banktree::new(0, bank);
        assert_eq!(banktree.working_bank().tick_height(), tick_height);
    }

    #[test]
    fn test_banktree_parent() {
        let bank = Bank::default();
        let mut banktree = Banktree::new(0, bank);
        let child_bank = Bank::new_from_parent(&banktree.working_bank());
        child_bank.register_tick(&Hash::default());
        let child_bank_id = 1;
        banktree.insert(child_bank_id, child_bank);
        banktree.set_working_bank_id(child_bank_id);
        assert_eq!(banktree.working_bank().tick_height(), child_bank_id);
    }
}
