//! The `ledger_cleanup_service` drops older ledger data to limit disk space usage

use crate::result::{Error, Result};
use crate::service::Service;
use solana_ledger::blocktree::Blocktree;
use solana_sdk::clock::DEFAULT_SLOTS_PER_EPOCH;
use solana_sdk::pubkey::Pubkey;
use std::string::ToString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::thread::{Builder, JoinHandle};
use std::time::Duration;

pub const DEFAULT_MAX_LEDGER_SLOTS: u64 = 3 * DEFAULT_SLOTS_PER_EPOCH;

pub struct LedgerCleanupService {
    t_cleanup: JoinHandle<()>,
}

impl LedgerCleanupService {
    pub fn new(
        slot_full_receiver: Receiver<(u64, Pubkey)>,
        blocktree: Arc<Blocktree>,
        max_ledger_slots: u64,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        info!(
            "LedgerCleanupService active. Max Ledger Slots {}",
            max_ledger_slots
        );
        let exit = exit.clone();
        let t_cleanup = Builder::new()
            .name("solana-ledger-cleanup".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) =
                    Self::cleanup_ledger(&slot_full_receiver, &blocktree, max_ledger_slots)
                {
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
        slot_full_receiver: &Receiver<(u64, Pubkey)>,
        blocktree: &Arc<Blocktree>,
        max_ledger_slots: u64,
    ) -> Result<()> {
        let (slot, _) = slot_full_receiver.recv_timeout(Duration::from_secs(1))?;
        if slot > max_ledger_slots {
            //cleanup
            blocktree.purge_slots(0, Some(slot - max_ledger_slots));
        }
        Ok(())
    }
}

impl Service for LedgerCleanupService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_cleanup.join()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use solana_ledger::blocktree::get_tmp_ledger_path;
    use solana_ledger::blocktree::make_many_slot_entries;
    use std::sync::mpsc::channel;

    #[test]
    fn test_cleanup() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let (shreds, _) = make_many_slot_entries(0, 50, 5);
        blocktree.insert_shreds(shreds, None).unwrap();
        let blocktree = Arc::new(blocktree);
        let (sender, receiver) = channel();

        //send a signal to kill slots 0-40
        sender.send((50, Pubkey::default())).unwrap();
        LedgerCleanupService::cleanup_ledger(&receiver, &blocktree, 10).unwrap();

        //check that 0-40 don't exist
        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, _)| assert!(slot > 40));

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }
}
