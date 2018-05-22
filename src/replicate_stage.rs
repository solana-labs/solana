//! The `replicate_stage` replicates transactions broadcast by the leader.

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

pub struct ReplicateStage {
    pub thread_hdl: JoinHandle<()>,
}

impl ReplicateStage {

    pub fn new(request_replicator: RequestReplicator, exit: Arc<AtomicBool>, window_receiver: streamer::BlobReceiver, blob_recycler: &packet::BlobRecycler) -> Self {
        let thread_hdl = spawn(move || loop {
            let e = request_replicator.replicate_requests(&window_receiver, &blob_recycler);
            if e.is_err() && s_exit.load(Ordering::Relaxed) {
                break;
            }
        });
        ReplicateStage{thread_hdl};
    }
}
