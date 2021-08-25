//! this module manages `cost tracker` for each bank, which is shared by banking_stage threads
//! (eg.,
//! clients); it releases `cost tracker` objects when no more client works on its bank.
//!
//! Notes:
//! 1. there are multiple threads/clients work on a bank, each bank has one cost_tracker;
//! 2. each banking_stage thread (client) can have one curernt bank at any given moment;
//! 3. cost_tracker should be released when its bank is no longer `current bank`;
//!
//! usgae:
//! test cases below
//!

use crate::{cost_model::CostModel, cost_tracker::CostTracker};
use dashmap::DashMap;
use solana_sdk::clock::Slot;
use std::sync::{Arc, RwLock};

// a simple struct in place of tuple as `value` of DashMap
#[derive(Debug)]
struct BankCostTrackerRecord {
    cost_tracker: Arc<RwLock<CostTracker>>,
    ref_count: u64,
}

impl BankCostTrackerRecord {
    fn ref_sub(mut self, v: u64) -> Self {
        self.ref_count = self.ref_count.saturating_sub(v);
        self
    }

    fn ref_add(mut self, v: u64) -> Self {
        self.ref_count = self.ref_count.saturating_add(v);
        self
    }

    fn cost_tracker(&self) -> Arc<RwLock<CostTracker>> {
        self.cost_tracker.clone()
    }
}

// tracks current slot for each client (eg banking_stage thread), maintains lifetime of
// cost_tracker for each slot. Release cost_tracker when its slot is no longer being worked by by
// any client
#[derive(Debug)]
pub struct BankCostTrackers {
    client_to_slot: DashMap<u32, Slot>,
    slot_to_record: DashMap<Slot, BankCostTrackerRecord>,
    cost_model: Arc<RwLock<CostModel>>,
}

impl BankCostTrackers {
    pub fn new(cost_model: Arc<RwLock<CostModel>>) -> Self {
        Self {
            client_to_slot: DashMap::new(),
            slot_to_record: DashMap::new(),
            cost_model,
        }
    }

    // the capacity could be the number of banking_stage thredas
    pub fn new_with_capacity(cost_model: Arc<RwLock<CostModel>>, capacity: usize) -> Self {
        Self {
            client_to_slot: DashMap::with_capacity(capacity),
            slot_to_record: DashMap::with_capacity(capacity),
            cost_model,
        }
    }

    pub fn get_cost_tracker(&self, slot: &Slot) -> Option<Arc<RwLock<CostTracker>>> {
        let rec = self.slot_to_record.get(slot)?;
        Some(rec.cost_tracker())
    }

    pub fn set_current_bank(&self, id: &u32, slot: &Slot) {
        match self.client_to_slot.get(id) {
            Some(existing_slot) => {
                if *existing_slot != *slot {
                    self.remove_slot(&existing_slot);
                    self.add_slot(slot);
                }
            }
            None => {
                self.client_to_slot.insert(*id, *slot);
                self.add_slot(slot);
            }
        }
    }

    // if slot exists, increases its ref_count then returns cost_tracker,
    // otherwise creates and adds it to map, before returning it
    fn add_slot(&self, slot: &Slot) {
        if self.slot_to_record.contains_key(slot) {
            self.slot_to_record
                .alter(slot, |_, record| record.ref_add(1));
        } else {
            let cost_tracker = Arc::new(RwLock::new(CostTracker::new(self.cost_model.clone())));
            self.slot_to_record.insert(
                *slot,
                BankCostTrackerRecord {
                    cost_tracker,
                    ref_count: 1,
                },
            );
        }
    }

    // reduce slot's ref_count, release it if ref_count down to zero
    fn remove_slot(&self, slot: &Slot) {
        self.slot_to_record
            .alter(slot, |_, record| record.ref_sub(1));
        self.slot_to_record
            .remove_if(slot, |_, record| record.ref_count == 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost_model::TransactionCost;
    use std::{
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
        thread::Builder,
    };

    #[test]
    fn test_bank_cost_trackers_base_case() {
        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let client_1: u32 = 1;
        let client_2: u32 = 2;
        let slot: Slot = 101;
        let cost: u64 = 10;
        let tx_cost = TransactionCost {
            execution_cost: cost,
            ..TransactionCost::default()
        };

        let bank_cost_trackers = BankCostTrackers::new(cost_model);

        // set current bank
        bank_cost_trackers.set_current_bank(&client_1, &slot);
        let cost_tracker_1 = bank_cost_trackers.get_cost_tracker(&slot).unwrap();
        assert_eq!(1, bank_cost_trackers.slot_to_record.len());
        assert_eq!(1, bank_cost_trackers.client_to_slot.len());
        // add some cost to the tracker
        assert_eq!(
            cost,
            cost_tracker_1.write().unwrap().try_add(&tx_cost).unwrap()
        );

        // get same cost tracker instance for same slot
        let cost_tracker_2 = bank_cost_trackers.get_cost_tracker(&slot).unwrap();
        // add cost again
        assert_eq!(
            cost * 2,
            cost_tracker_2.write().unwrap().try_add(&tx_cost).unwrap()
        );

        // get same cost tracker if second client sets to same slot
        bank_cost_trackers.set_current_bank(&client_2, &slot);
        let cost_tracker_3 = bank_cost_trackers.get_cost_tracker(&slot).unwrap();
        assert_eq!(1, bank_cost_trackers.slot_to_record.len());
        assert_eq!(2, bank_cost_trackers.client_to_slot.len());
        // add some cost to the tracker
        assert_eq!(
            cost * 3,
            cost_tracker_3.write().unwrap().try_add(&tx_cost).unwrap()
        );
    }

    #[test]
    fn test_bank_cost_trackers_concurrency() {
        // thread works on slot_1 should not be effacted by other thread works on slot_2,
        // which was the case when cost tracker being shared with threads - the case this
        // module solves
        solana_logger::setup();

        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let client_1: u32 = 1;
        let client_2: u32 = 2;
        let slot_1: Slot = 1;
        let slot_2: Slot = 2;
        let cost: u64 = 10;
        let tx_cost = TransactionCost {
            execution_cost: cost,
            ..TransactionCost::default()
        };
        let exit = Arc::new(AtomicBool::new(false));
        let count = Arc::new(AtomicU64::new(0));

        let bank_cost_trackers = Arc::new(BankCostTrackers::new(cost_model));
        {
            // set init cost for bank 1
            bank_cost_trackers.set_current_bank(&client_1, &slot_1);
            let cost_tracker_1 = bank_cost_trackers.get_cost_tracker(&slot_1).unwrap();
            assert!(cost_tracker_1.write().unwrap().try_add(&tx_cost).is_ok());
        }

        let exit_1 = exit.clone();
        let bank_cost_trackers_1 = bank_cost_trackers.clone();
        let t1 = Builder::new()
            .name("repeatedly_read_slot_1".to_string())
            .spawn(move || loop {
                assert_eq!(
                    cost,
                    bank_cost_trackers_1
                        .get_cost_tracker(&slot_1)
                        .unwrap()
                        .read()
                        .unwrap()
                        .get_block_cost()
                );
                if exit_1.load(Ordering::Relaxed) {
                    break;
                }
            })
            .unwrap();

        let exit_2 = exit.clone();
        let count_2 = count.clone();
        let bank_cost_trackers_2 = bank_cost_trackers;
        let t2 = Builder::new()
            .name("repeatedly_write_and_reset_slot_2".to_string())
            .spawn(move || loop {
                bank_cost_trackers_2.set_current_bank(&client_2, &slot_2);
                let cost_tracker_t2 = bank_cost_trackers_2.get_cost_tracker(&slot_2).unwrap();
                cost_tracker_t2
                    .write()
                    .unwrap()
                    .reset_if_new_bank(count_2.fetch_add(1, Ordering::SeqCst) % 2);
                assert!(cost_tracker_t2.write().unwrap().try_add(&tx_cost).is_ok());
                if exit_2.load(Ordering::Relaxed) {
                    break;
                }
            })
            .unwrap();

        while count.load(Ordering::Relaxed) < 100 {}

        exit.store(true, Ordering::Relaxed);
        t1.join().unwrap();
        t2.join().unwrap();
    }

    #[test]
    fn test_bank_cost_trackers_purge_old_records() {
        solana_logger::setup();

        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let client_1: u32 = 1;
        let client_2: u32 = 2;
        let slot_1: Slot = 101;
        let slot_2: Slot = 102;

        let bank_cost_trackers = Arc::new(BankCostTrackers::new(cost_model));

        // client 1 declare slot 1, now it has one record
        bank_cost_trackers.set_current_bank(&client_1, &slot_1);
        assert_eq!(1, bank_cost_trackers.slot_to_record.len());
        assert_eq!(1, bank_cost_trackers.client_to_slot.len());
        assert!(bank_cost_trackers.get_cost_tracker(&slot_1).is_some());

        // client 2 declare slot 2, now it has two records
        bank_cost_trackers.set_current_bank(&client_2, &slot_2);
        assert_eq!(2, bank_cost_trackers.slot_to_record.len());
        assert_eq!(2, bank_cost_trackers.client_to_slot.len());
        assert!(bank_cost_trackers.get_cost_tracker(&slot_1).is_some());
        assert!(bank_cost_trackers.get_cost_tracker(&slot_2).is_some());

        // client 1 declare slot 1 again, makes no change
        bank_cost_trackers.set_current_bank(&client_1, &slot_1);
        assert_eq!(2, bank_cost_trackers.slot_to_record.len());
        assert_eq!(2, bank_cost_trackers.client_to_slot.len());
        assert!(bank_cost_trackers.get_cost_tracker(&slot_1).is_some());
        assert!(bank_cost_trackers.get_cost_tracker(&slot_2).is_some());

        // client 1 declare slot 2, the slot_1 would be freed
        bank_cost_trackers.set_current_bank(&client_1, &slot_2);
        assert_eq!(1, bank_cost_trackers.slot_to_record.len());
        assert_eq!(2, bank_cost_trackers.client_to_slot.len());
        assert!(bank_cost_trackers.get_cost_tracker(&slot_1).is_none());
        assert!(bank_cost_trackers.get_cost_tracker(&slot_2).is_some());
    }
}
