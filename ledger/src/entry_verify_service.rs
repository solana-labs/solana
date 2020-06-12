use crate::bank_forks::BankForks;
use crate::entry::Entry;
use crate::entry::EntrySlice;
use crate::unverified_blocks::UnverifiedBlocks;
use solana_sdk::clock::Slot;
use std::collections::HashMap;
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
        slot_receiver: Receiver<(Slot, Vec<Entry>, u128)>,
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
        slot_receiver: &Receiver<(Slot, Vec<Entry>, u128)>,
        bank_forks: &Arc<RwLock<BankForks>>,
        slot_verify_results: &Arc<RwLock<HashMap<Slot, bool>>>,
        unverified_blocks: &mut UnverifiedBlocks,
    ) -> Result<(), RecvTimeoutError> {
        let root_slot = bank_forks.read().unwrap().root();

        unverified_blocks.set_root(root_slot);

        while let Ok((slot, entries, weight)) = slot_receiver.try_recv() {
            let parent = bank_forks
                .read()
                .unwrap()
                .get(slot)
                .unwrap()
                .parent()
                .unwrap();
            let parent_slot = parent.slot();
            let parent_hash = parent.hash();
            unverified_blocks.add_unverified_block(slot, parent_slot, entries, weight, parent_hash);
        }
        if let Some(heaviest_leaf) = unverified_blocks.next_heaviest_leaf() {
            let heaviest_ancestors = unverified_blocks.get_unverified_ancestors(heaviest_leaf);
            if let Some(heaviest_slot) = heaviest_ancestors.iter().next() {
                let block_info = unverified_blocks
                    .unverified_blocks
                    .get(heaviest_slot)
                    .unwrap();
                let verify_result = block_info.entries.verify(&block_info.parent_hash);
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
