//! The `replicate_stage` replicates transactions broadcast by the leader.

use bank::Bank;
use ledger;
use packet;
use result::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use streamer;

pub struct ReplicateStage {
    pub thread_hdl: JoinHandle<()>,
}

impl ReplicateStage {
    /// Process verified blobs, already in order
    fn replicate_requests(
        bank: &Arc<Bank>,
        verified_receiver: &streamer::BlobReceiver,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let blobs = verified_receiver.recv_timeout(timer)?;
        let entries = ledger::reconstruct_entries_from_blobs(&blobs);
        let res = bank.process_entries(entries);
        if res.is_err() {
            error!("process_entries {} {:?}", blobs.len(), res);
        }
        res?;
        for blob in blobs {
            blob_recycler.recycle(blob);
        }
        Ok(())
    }

    pub fn new(
        bank: Arc<Bank>,
        exit: Arc<AtomicBool>,
        window_receiver: streamer::BlobReceiver,
        blob_recycler: packet::BlobRecycler,
    ) -> Self {
        let thread_hdl = spawn(move || loop {
            let e = Self::replicate_requests(&bank, &window_receiver, &blob_recycler);
            if e.is_err() && exit.load(Ordering::Relaxed) {
                break;
            }
        });
        ReplicateStage { thread_hdl }
    }
}
