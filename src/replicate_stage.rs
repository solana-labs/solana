//! The `replicate_stage` replicates transactions broadcast by the leader.

use bank::Bank;
use ledger;
use result::{Error, Result};
use service::Service;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use streamer::BlobReceiver;

pub struct ReplicateStage {
    thread_hdl: JoinHandle<()>,
}

impl ReplicateStage {
    /// Process entry blobs, already in order
    fn replicate_requests(bank: &Arc<Bank>, blob_receiver: &BlobReceiver) -> Result<()> {
        let timer = Duration::new(1, 0);
        let blobs = blob_receiver.recv_timeout(timer)?;
        let blobs_len = blobs.len();
        let entries = ledger::reconstruct_entries_from_blobs(blobs)?;
        let res = bank.process_entries(entries);
        if res.is_err() {
            error!("process_entries {} {:?}", blobs_len, res);
        }
        res?;
        Ok(())
    }

    pub fn new(bank: Arc<Bank>, window_receiver: BlobReceiver) -> Self {
        let thread_hdl = Builder::new()
            .name("solana-replicate-stage".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::replicate_requests(&bank, &window_receiver) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap();
        ReplicateStage { thread_hdl }
    }
}

impl Service for ReplicateStage {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        vec![self.thread_hdl]
    }

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
