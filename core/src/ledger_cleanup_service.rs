//! The `ledger_cleanup_service` drops older ledger data to limit disk space usage

use crate::result::{Error, Result};
use solana_ledger::blocktree::Blocktree;
use solana_metrics::datapoint_debug;
use solana_sdk::clock::Slot;
use std::string::ToString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::thread::{Builder, JoinHandle};
use std::time::Duration;

// This is chosen to allow enough time for
// - To try and keep the RocksDB size under 128GB at 50k tps (100 slots take ~2GB).
// - A validator to download a snapshot from a peer and boot from it
// - To make sure that if a validator needs to reboot from its own snapshot, it has enough slots locally
//   to catch back up to where it was when it stopped
pub const MAX_LEDGER_SLOTS: u64 = 6400;
// Remove a fixed number of slots at a time, it's more efficient than doing it one-by-one
pub const DEFAULT_PURGE_BATCH_SIZE: u64 = 256;

pub struct LedgerCleanupService {
    t_cleanup: JoinHandle<()>,
}

impl LedgerCleanupService {
    pub fn new(
        new_root_receiver: Receiver<Slot>,
        blocktree: Arc<Blocktree>,
        max_ledger_slots: u64,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        info!(
            "LedgerCleanupService active. Max Ledger Slots {}",
            max_ledger_slots
        );
        let exit = exit.clone();
        let mut next_purge_batch = max_ledger_slots;
        let t_cleanup = Builder::new()
            .name("solana-ledger-cleanup".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = Self::cleanup_ledger(
                    &new_root_receiver,
                    &blocktree,
                    max_ledger_slots,
                    &mut next_purge_batch,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => info!("Error from cleanup_ledger: {:?}", e),
                    }
                }
            })
            .unwrap();
        Self { t_cleanup }
    }

    fn cleanup_ledger(
        new_root_receiver: &Receiver<Slot>,
        blocktree: &Arc<Blocktree>,
        max_ledger_slots: u64,
        next_purge_batch: &mut u64,
    ) -> Result<()> {
        let disk_utilization_pre = blocktree.storage_size();

        let root = new_root_receiver.recv_timeout(Duration::from_secs(1))?;
        if root > *next_purge_batch {
            //cleanup
            blocktree.purge_slots(0, Some(root - max_ledger_slots));
            *next_purge_batch += DEFAULT_PURGE_BATCH_SIZE;
        }

        let disk_utilization_post = blocktree.storage_size();

        if let (Ok(disk_utilization_pre), Ok(disk_utilization_post)) =
            (disk_utilization_pre, disk_utilization_post)
        {
            datapoint_debug!(
                "ledger_disk_utilization",
                ("disk_utilization_pre", disk_utilization_pre as i64, i64),
                ("disk_utilization_post", disk_utilization_post as i64, i64),
                (
                    "disk_utilization_delta",
                    (disk_utilization_pre as i64 - disk_utilization_post as i64),
                    i64
                )
            );
        }

        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_cleanup.join()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use solana_ledger::blocktree::make_many_slot_entries;
    use solana_ledger::get_tmp_ledger_path;
    use std::sync::mpsc::channel;

    #[test]
    fn test_cleanup() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let (shreds, _) = make_many_slot_entries(0, 50, 5);
        blocktree.insert_shreds(shreds, None, false).unwrap();
        let blocktree = Arc::new(blocktree);
        let (sender, receiver) = channel();

        //send a signal to kill slots 0-40
        let mut next_purge_slot = 0;
        sender.send(50).unwrap();
        LedgerCleanupService::cleanup_ledger(&receiver, &blocktree, 10, &mut next_purge_slot)
            .unwrap();

        //check that 0-40 don't exist
        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, _)| assert!(slot > 40));

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_compaction() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Arc::new(Blocktree::open(&blocktree_path).unwrap());

        let n = 10_000;
        let batch_size = 100;
        let batches = n / batch_size;
        let max_ledger_slots = 100;

        for i in 0..batches {
            let (shreds, _) = make_many_slot_entries(i * batch_size, batch_size, 1);
            blocktree.insert_shreds(shreds, None, false).unwrap();
        }

        let u1 = blocktree.storage_size().unwrap() as f64;

        // send signal to cleanup slots
        let (sender, receiver) = channel();
        sender.send(n).unwrap();
        let mut next_purge_batch = 0;
        LedgerCleanupService::cleanup_ledger(
            &receiver,
            &blocktree,
            max_ledger_slots,
            &mut next_purge_batch,
        )
        .unwrap();

        thread::sleep(Duration::from_secs(2));

        let u2 = blocktree.storage_size().unwrap() as f64;

        assert!(u2 < u1, "insufficient compaction! pre={},post={}", u1, u2,);

        // check that early slots don't exist
        let max_slot = n - max_ledger_slots;
        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, _)| assert!(slot > max_slot));

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }
}
