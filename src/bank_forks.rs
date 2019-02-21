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

    // TODO: use the bank's own ID instead of receiving a parameter
    pub fn insert(&mut self, bank_id: u64, bank: Bank) {
        let mut bank = Arc::new(bank);
        self.banks.insert(bank_id, bank.clone());

        // TODO: this really only needs to look at the first
        //  parent if we're always calling insert()
        //  when we construct a child bank
        while let Some(parent) = bank.parent() {
            self.banks.remove(&parent.id());
            bank = parent;
        }
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
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_bank_forks_root() {
        let bank = Bank::default();
        let bank_forks = BankForks::new(0, bank);
        assert_eq!(bank_forks.working_bank().tick_height(), 0);
    }

    #[test]
    fn test_bank_forks_parent() {
        let bank = Bank::default();
        let mut bank_forks = BankForks::new(0, bank);
        let child_bank = Bank::new_from_parent(&bank_forks.working_bank(), &Pubkey::default());
        child_bank.register_tick(&Hash::default());
        bank_forks.insert(1, child_bank);
        bank_forks.set_working_bank_id(1);
        assert_eq!(bank_forks.working_bank().tick_height(), 1);
    }
}
