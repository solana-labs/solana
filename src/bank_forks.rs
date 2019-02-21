//! The `bank_forks` module implments BankForks a DAG of checkpointed Banks

use solana_runtime::bank::Bank;
use std::collections::HashMap;
use std::sync::Arc;

pub struct BankForks {
    working_bank_id: u64,
    banks: HashMap<u64, Arc<Bank>>,
}

impl BankForks {
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
    use solana_sdk::genesis_block::GenesisBlock;

    #[test]
    fn test_bank_forks_root() {
        let bank = Bank::default();
        let tick_height = bank.tick_height();
        let bank_forks = BankForks::new(0, bank);
        assert_eq!(bank_forks.working_bank().tick_height(), tick_height);
    }

    #[test]
    fn test_bank_forks_parent() {
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks.working_bank());
        child_bank.register_tick(&Hash::default());
        let child_bank_id = 1;
        bank_forks.insert(child_bank_id, child_bank);
        bank_forks.set_working_bank_id(child_bank_id);
        assert_eq!(bank_forks.working_bank().tick_height(), child_bank_id);
    }
}
