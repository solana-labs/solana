use crate::{
    bank_forks::BankForks,
    entry::{Entry, EntrySlice, VerifyRecyclers},
    unverified_blocks::UnverifiedBlocks,
};
use solana_sdk::clock::Slot;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::{Receiver, RecvTimeoutError, Sender},
    sync::Arc,
    sync::RwLock,
    thread::{self, Builder, JoinHandle},
    time::Instant,
};

pub type VerifySlotSender = Sender<Vec<(Slot, Vec<Entry>, u128)>>;
pub type VerifySlotReceiver = Receiver<Vec<(Slot, Vec<Entry>, u128)>>;

pub struct EntryVerifyService {
    t_verify: JoinHandle<()>,
}

impl EntryVerifyService {
    pub fn new(
        slot_receiver: VerifySlotReceiver,
        bank_forks: Arc<RwLock<BankForks>>,
        slot_verify_results: Arc<RwLock<HashMap<Slot, bool>>>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();

        let t_verify = Builder::new()
            .name("solana-entry-verify".to_string())
            .spawn(move || {
                let mut unverified_blocks = UnverifiedBlocks::default();
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Err(e) = Self::verify_entries(
                        &slot_receiver,
                        &bank_forks,
                        &slot_verify_results,
                        &mut unverified_blocks,
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
        slot_receiver: &VerifySlotReceiver,
        bank_forks: &Arc<RwLock<BankForks>>,
        slot_verify_results: &Arc<RwLock<HashMap<Slot, bool>>>,
        unverified_blocks: &mut UnverifiedBlocks,
    ) -> Result<(), RecvTimeoutError> {
        let root_slot = bank_forks.read().unwrap().root();

        unverified_blocks.set_root(root_slot);

        while let Ok(slot_entries) = slot_receiver.try_recv() {
            for (slot, entries, weight) in slot_entries {
                // If slot bank doesn't exist, then it must have been
                // pruned by `set_root` and verification is no longer necessary
                {
                    // Hold the lock so that `set_root` doesn't get called
                    // in the middle of this logic
                    let w_bank_forks = bank_forks.write().unwrap();
                    if let Some(bank) = w_bank_forks.get(slot) {
                        let parent_bank = bank.parent().expect(
                            "Unverified slot can't be the root, so
                        parent must exist",
                        );
                        let parent_slot = parent_bank.slot();
                        let parent_hash = parent_bank.hash();
                        unverified_blocks.add_unverified_block(
                            slot,
                            parent_slot,
                            entries,
                            weight,
                            parent_hash,
                        );
                    }
                }
            }
        }
        if let Some(heaviest_leaf) = unverified_blocks.next_heaviest_leaf() {
            let heaviest_ancestors = unverified_blocks.get_unverified_ancestors(heaviest_leaf);
            if let Some(heaviest_slot) = heaviest_ancestors.iter().next() {
                let start = Instant::now();
                let block_info = unverified_blocks
                    .unverified_blocks
                    .get(heaviest_slot)
                    .unwrap();
                let verify_result = block_info
                    .entries
                    .start_verify(&block_info.parent_hash, VerifyRecyclers::default())
                    .finish_verify(&block_info.entries);
                datapoint_info!(
                    "verify_poh_elapsed",
                    ("slot", *heaviest_slot, i64),
                    ("elapsed_micros", start.elapsed().as_micros(), i64)
                );
                slot_verify_results
                    .write()
                    .unwrap()
                    .insert(*heaviest_slot, verify_result);
            }
        }
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_verify.join()
    }
}
