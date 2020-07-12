use crate::{
    heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
    repair_weighted_traversal::RepairWeightTraversal, unverified_blocks::UnverifiedBlocks,
};
use crossbeam_channel::{unbounded, Receiver, SendError, Sender};
use solana_ledger::entry::{Entry, EntrySlice, VerifyOption, VerifyRecyclers};
use solana_runtime::{bank::Bank, bank_forks::BankForks};
use solana_sdk::{clock::Slot, hash::Hash};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        {Arc, RwLock},
    },
    thread::{self, Builder, JoinHandle},
};

pub type VerifySlotSender = Sender<Vec<SlotEntriesToVerify>>;
pub type VerifySlotReceiver = Receiver<Vec<SlotEntriesToVerify>>;
pub type VerifyResult = (Slot, bool);
pub type VerifySlotResultSender = Sender<VerifyResult>;
pub type VerifySlotResultReceiver = Receiver<VerifyResult>;

pub struct SlotEntriesToVerify {
    pub slot: Slot,
    pub entries: Vec<Entry>,
    pub parent_slot: Slot,
    pub parent_hash: Hash,
}

pub struct EntryVerifyService {
    t_verify: JoinHandle<()>,
}

impl EntryVerifyService {
    pub fn new(
        slot_receiver: VerifySlotReceiver,
        bank_forks: Arc<RwLock<BankForks>>,
        exit: &Arc<AtomicBool>,
        heaviest_subtree_fork_choice: Arc<RwLock<HeaviestSubtreeForkChoice>>,
        frozen_banks: &[Arc<Bank>],
    ) -> (Self, VerifySlotResultReceiver) {
        let exit = exit.clone();
        let (verify_slot_result_sender, verify_slot_result_receiver) = unbounded();

        // Because the `frozen_banks` are sorted, the
        // first frozen bank must be the root slot
        let root_slot = bank_forks.read().unwrap().root();
        assert_eq!(root_slot, frozen_banks[0].slot());
        let mut unverified_blocks = UnverifiedBlocks::new(root_slot);

        // Add these slots that are present at startup for bookkeeping about
        // fork structure. These slots won't actually be verified again
        let mut last_slot = 0;
        // Skip the root slot since we initialized `unverified_blocks` already
        // with the root slot
        for bank in frozen_banks.iter().skip(1) {
            // `frozen_banks` must be sorted
            assert!(bank.slot() >= last_slot);
            last_slot = bank.slot();
            let parent_bank = bank
                .parent()
                .expect("All non-rooted banks must have a parent");
            unverified_blocks.add_unverified_block(
                bank.slot(),
                parent_bank.slot(),
                vec![],
                parent_bank.last_blockhash(),
            );
        }
        let t_verify = Builder::new()
            .name("solana-entry-verify".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }

                if let Err(e) = Self::verify_entries(
                    &slot_receiver,
                    &bank_forks,
                    &verify_slot_result_sender,
                    &mut unverified_blocks,
                    &heaviest_subtree_fork_choice,
                ) {
                    match e {
                        SendError(_) => break,
                    }
                }
            })
            .unwrap();
        (Self { t_verify }, verify_slot_result_receiver)
    }

    fn verify_entries(
        slot_receiver: &VerifySlotReceiver,
        bank_forks: &Arc<RwLock<BankForks>>,
        verify_slot_result_sender: &VerifySlotResultSender,
        unverified_blocks: &mut UnverifiedBlocks,
        // Warning: Do not grab BankForks lock and then try to grab this
        // lock (must grab this lock first). Otherwise will deadlock with
        // ReplayStage's `fork_choice.select_forks()` when `fork_choice`
        // is of type `HeaviestSubtreeForkChoice`.
        heaviest_subtree_fork_choice: &RwLock<HeaviestSubtreeForkChoice>,
    ) -> Result<(), SendError<VerifyResult>> {
        // Read out any pending new blocks that need verification
        while let Ok(received) = slot_receiver.try_recv() {
            for slot_entries_to_verify in received {
                let SlotEntriesToVerify {
                    slot,
                    entries,
                    parent_slot,
                    parent_hash,
                } = slot_entries_to_verify;
                // This is only safe to call if `slot_entries_to_verify.parent_slot`
                // exists in `unverified_blocks`. However, we know this must
                // be true because:
                //
                // 1) `parent_slot` must have been added earlier
                // to `unverified_blocks` since ReplayStage always pushes
                // ancestors before descendants to the `slot_receiver` channel
                //
                // 2) By this point we know means the parent could not have been purged
                // from `unverified_blocks` due to a verification failure. Thus
                // the only other possible purge is if `unverified_blocks.set_root()`
                // purged this slot in an earlier call to this function.
                //
                // 3) However, we also know any earler calls to this function could
                // not have purged this slot through `unverified_blocks.set_root()`. This
                // is because we checked above that `parent_slot` must exist in `bank_forks`
                // (because `slot` exists in `bank_forks` and is not a root), so
                // any earlier calls to `unverified_blocks.set_root()` before that checlk
                // could not have purged a slot that was not yet purged in `bank_forks`
                unverified_blocks.add_unverified_block(slot, parent_slot, entries, parent_hash);
            }
        }

        // Purge any blocks that are outdated. Any root in `BankForks` must exist in
        // `unverified_blocks` by now because a root cannot be set without being
        // verified
        let root = bank_forks.read().unwrap().root();
        unverified_blocks.set_root(root);

        // Get the best unverified block
        let best_unverified = {
            let r_heaviest_subtree_fork_choice = heaviest_subtree_fork_choice.read().unwrap();
            // Based on the traversal, should always choose parents before children
            let weighted_traversal = RepairWeightTraversal::new(&r_heaviest_subtree_fork_choice);
            unverified_blocks.get_heaviest_block(weighted_traversal)
        };

        if let Some((heaviest_slot, parent_hash, unverified_entries)) = best_unverified {
            let mut verifier = unverified_entries.start_verify(
                &parent_hash,
                VerifyRecyclers::default(),
                VerifyOption::PohOnly,
            );

            let verify_result = verifier.finish_verify(&unverified_entries);
            datapoint_info!(
                "verify_poh_elapsed",
                ("slot", heaviest_slot, i64),
                ("elapsed", verifier.poh_duration_us(), i64)
            );

            verify_slot_result_sender.send((heaviest_slot, verify_result))?;

            info!(
                "Verifying slot: {}, num_entries: {}, parent_hash: {}, result: {}",
                heaviest_slot,
                unverified_entries.len(),
                parent_hash,
                verify_result,
            );
        }
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_verify.join()
    }
}
