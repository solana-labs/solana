//! The `request_replicator` is part of `replicator_stage` which replicates transactions broadcast
//! by the leader.

use bank::Bank;
use banking_stage::BankingStage;
use crdt::{Crdt, ReplicatedData};
use hash::Hash;
use ledger;
use packet;
use record_stage::RecordStage;
use result::Result;
use sig_verify_stage::SigVerifyStage;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use streamer;
use write_stage::WriteStage;

pub struct RequestReplicator {
    bank: Arc<Bank>,
}

impl Tvu {
    /// Create a new Tvu that wraps the given Bank.
    pub fn new(bank: Bank) -> Self {
        RequestReplicator {
            bank: Arc::new(bank),
        }
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
