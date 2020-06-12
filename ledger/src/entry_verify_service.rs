use crate::bank_forks::BankForks;
use crate::entry::Entry;
use crate::entry::EntrySlice;
use solana_sdk::clock::Slot;
use solana_sdk::hash::Hash;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::sync::RwLock;
use std::thread::{self, Builder, JoinHandle};

pub struct EntryVerifyService {
    t_verify: JoinHandle<()>,
}

impl EntryVerifyService {
    pub fn new(
        slot_receiver: Receiver<(Slot, Vec<Entry>, u64)>,
        bank_forks: Arc<RwLock<BankForks>>,
        slots_verified: Arc<RwLock<HashMap<Slot, bool>>>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();

        let t_verify = Builder::new()
            .name("solana-entry-verify".to_string())
            .spawn(move || {
                let mut slots_map = BTreeMap::new();
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Err(e) = Self::verify_entries(
                        &slot_receiver,
                        &bank_forks,
                        &slots_verified,
                        &mut slots_map,
                    ) {
                        match e {
                            RecvTimeoutError::Disconnected => break,
                            RecvTimeoutError::Timeout => (),
                        }
                    }
                }
            })
            .unwrap();
        Self { t_verify }
    }

    fn verify_entries(
        slot_receiver: &Receiver<(Slot, Vec<Entry>, u64)>,
        bank_forks: &Arc<RwLock<BankForks>>,
        slots_verified: &Arc<RwLock<HashMap<Slot, bool>>>,
        slots_map: &mut BTreeMap<u64, (Slot, Vec<Entry>)>,
    ) -> Result<(), RecvTimeoutError> {
        let root_slot = bank_forks.read().unwrap().root();
        let mut slots_to_remove = Vec::new();
        for (weight, e) in slots_map.iter() {
            if e.0 <= root_slot {
                slots_to_remove.push(*weight);
            }
        }
        for weight in slots_to_remove {
            slots_map.remove(&weight);
        }
        while let Ok(slots) = slot_receiver.try_recv() {
            slots_map.insert(slots.2, (slots.0, slots.1));
        }
        if let Some((_weight, (slot, _entries))) = slots_map.iter().next() {
            let mut heaviest_bank = bank_forks.read().unwrap().get(*slot).unwrap().clone();
            while slots_map.contains_key(&heaviest_bank.slot()) {
                heaviest_bank = heaviest_bank.parent().unwrap();
            }
            let (slot, entries) = slots_map.get(&heaviest_bank.slot()).unwrap();
            let result = entries.verify(&Hash::default());
            slots_verified.write().unwrap().insert(*slot, result);
        }
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_verify.join()
    }
}
