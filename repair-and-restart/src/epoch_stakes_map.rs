use {
    solana_program::clock::Epoch,
    solana_runtime::{
        bank::Bank,
        epoch_stakes::EpochStakes,
    },
    solana_sdk::clock::Slot,
    std::{
        collections::HashSet,
        sync::Arc,
    },
};

pub struct EpochStakesMap {
    missing_epochs: HashSet<Epoch>,
    bank_with_all_epochstakes: Arc<Bank>,
}

impl EpochStakesMap {
    pub(crate) fn new(bank: Arc<Bank>) -> Self {
        Self {
            missing_epochs: HashSet::new(),
            bank_with_all_epochstakes: bank,
        }
    }

    pub(crate) fn has_missing_epochs(&self) -> bool {
        return !self.missing_epochs.is_empty();
    }

    pub(crate) fn update_bank(&mut self, new_bank: Arc<Bank>) {
        if self.missing_epochs.is_empty() {
            return;
        }
        if self.missing_epochs.contains(&new_bank.epoch()) {
            self.bank_with_all_epochstakes = new_bank;
        }
    }

    pub fn epoch_stakes(&mut self, slot: Option<Slot>) -> &EpochStakes {
        let my_epoch = self.bank_with_all_epochstakes.epoch();
        match slot {
            Some(my_slot) => {
                let slot_epoch = self.bank_with_all_epochstakes.epoch_schedule().get_epoch(my_slot);
                match self.bank_with_all_epochstakes.epoch_stakes(slot_epoch) {
                    Some(new_map) => new_map,
                    None => {
                        self.missing_epochs.insert(slot_epoch);
                        self.bank_with_all_epochstakes.epoch_stakes(my_epoch).unwrap()
                    }
                }
            }
            None => self.bank_with_all_epochstakes.epoch_stakes(my_epoch).unwrap(),
        }
    }
}