//! The `bank_forks` module implments BankForks a DAG of checkpointed Banks

use crate::bank::Bank;
use std::collections::HashMap;
use std::sync::Arc;

pub struct BankForks {
    working_bank_id: u64,
    banks: HashMap<u64, Arc<Bank>>,
}

impl BankForks {
    pub fn new(bank: Bank) -> Self {
        let mut banks = HashMap::new();
        let working_bank_id = bank.tick_height();
        banks.insert(working_bank_id, Arc::new(bank));
        Self {
            working_bank_id,
            banks,
        }
    }

    pub fn working_bank(&self) -> Arc<Bank> {
        self.banks[&self.working_bank_id].clone()
    }

    pub fn finalized_bank(&self) -> Arc<Bank> {
        let mut bank = self.working_bank();
        while let Some(parent) = bank.parent() {
            bank = parent;
        }
        bank
    }

    pub fn insert(&mut self, bank: Bank) -> u64 {
        let bank_id = bank.tick_height();
        self.banks.insert(bank_id, Arc::new(bank));
        bank_id
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
    fn test_bank_forks_root() {
        let bank = Bank::default();
        let tick_height = bank.tick_height();
        let bank_forks = BankForks::new(bank);
        assert_eq!(bank_forks.working_bank().tick_height(), tick_height);
        assert_eq!(bank_forks.finalized_bank().tick_height(), tick_height);
    }

    #[test]
    fn test_bank_forks_parent() {
        let bank = Bank::default();
        let finalized_bank_id = bank.tick_height();
        let mut bank_forks = BankForks::new(bank);
        let child_bank = Bank::new_from_parent(&bank_forks.working_bank());
        child_bank.register_tick(&Hash::default());
        let child_bank_id = bank_forks.insert(child_bank);
        bank_forks.set_working_bank_id(child_bank_id);
        assert_eq!(bank_forks.working_bank().tick_height(), child_bank_id);
        assert_eq!(bank_forks.finalized_bank().tick_height(), finalized_bank_id);
    }
}
