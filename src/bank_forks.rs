//! The `bank_forks` module implments BankForks a DAG of checkpointed Banks

use solana_runtime::bank::Bank;
use std::collections::HashMap;
use std::ops::Index;
use std::sync::Arc;

pub struct BankForks {
    banks: HashMap<u64, Arc<Bank>>,
    working_bank: Arc<Bank>,
}

impl Index<u64> for BankForks {
    type Output = Arc<Bank>;
    fn index(&self, bank_id: u64) -> &Arc<Bank> {
        &self.banks[&bank_id]
    }
}

impl BankForks {
    pub fn new(bank_id: u64, bank: Bank) -> Self {
        let mut banks = HashMap::new();
        let working_bank = Arc::new(bank);
        banks.insert(bank_id, working_bank.clone());
        Self {
            banks,
            working_bank,
        }
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
    pub fn insert(&mut self, bank_id: u64, bank: Bank) {
        let mut bank = Arc::new(bank);
        self.banks.insert(bank_id, bank.clone());

        if bank_id > self.working_bank.slot() {
            self.working_bank = bank.clone()
        }

        // TODO: this really only needs to look at the first
        //  parent if we're always calling insert()
        //  when we construct a child bank
        while let Some(parent) = bank.parent() {
            self.banks.remove(&parent.slot());
            bank = parent;
        }
    }

    // TODO: really want to kill this...
    pub fn working_bank(&self) -> Arc<Bank> {
        self.working_bank.clone()
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
}
