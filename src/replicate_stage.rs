//! The `replicate_stage` replicates transactions broadcast by the leader.

use bank::Bank;
use ledger;
use packet::BlobRecycler;
use result::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{Builder, JoinHandle};
use std::time::Duration;
use streamer::BlobReceiver;

pub struct ReplicateStage {
    pub thread_hdl: JoinHandle<()>,
}

impl ReplicateStage {
    /// Process entry blobs, already in order
    fn replicate_requests(
        bank: &Arc<Bank>,
        blob_receiver: &BlobReceiver,
        blob_recycler: &BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let blobs = blob_receiver.recv_timeout(timer)?;
        let blobs_len = blobs.len();
        let entries = ledger::reconstruct_entries_from_blobs(blobs, &blob_recycler)?;
        let res = bank.process_entries(entries);
        if res.is_err() {
            error!("process_entries {} {:?}", blobs_len, res);
        }
        res?;
        Ok(())
    }

    pub fn new(
        bank: Arc<Bank>,
        exit: Arc<AtomicBool>,
        window_receiver: BlobReceiver,
        blob_recycler: BlobRecycler,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solana-replicate-stage".to_string())
            .spawn(move || loop {
                let e = Self::replicate_requests(&bank, &window_receiver, &blob_recycler);
                if e.is_err() && exit.load(Ordering::Relaxed) {
                    break;
                }
            })
            .unwrap();
        ReplicateStage { thread_hdl }
    }
}
