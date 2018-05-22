//! The `request_replicator` is part of `replicator_stage` which replicates transactions broadcast
//! by the leader.

use bank::Bank;
use ledger;
use packet;
use result::Result;
use std::sync::Arc;
use std::time::Duration;
use streamer;

pub struct RequestReplicator {
    bank: Arc<Bank>,
}

impl RequestReplicator {
    /// Create a new Tvu that wraps the given Bank.
    pub fn new(bank: Arc<Bank>) -> Self {
        RequestReplicator { bank: bank }
    }

    /// Process verified blobs, already in order
    pub fn replicate_requests(
        &self,
        verified_receiver: &streamer::BlobReceiver,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let blobs = verified_receiver.recv_timeout(timer)?;
        let entries = ledger::reconstruct_entries_from_blobs(&blobs);
        let res = self.bank.process_verified_entries(entries);
        if res.is_err() {
            error!("process_verified_entries {} {:?}", blobs.len(), res);
        }
        res?;
        for blob in blobs {
            blob_recycler.recycle(blob);
        }
        Ok(())
    }
}
