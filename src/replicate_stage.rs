//! The `replicate_stage` replicates transactions broadcast by the leader.

use packet;
use request_replicator::RequestReplicator;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{spawn, JoinHandle};
use streamer;

pub struct ReplicateStage {
    pub thread_hdl: JoinHandle<()>,
}

impl ReplicateStage {

    pub fn new(request_replicator: RequestReplicator, exit: Arc<AtomicBool>, window_receiver: streamer::BlobReceiver, blob_recycler: packet::BlobRecycler) -> Self {
        let thread_hdl = spawn(move || loop {
            let e = request_replicator.replicate_requests(&window_receiver, &blob_recycler);
            if e.is_err() && exit.load(Ordering::Relaxed) {
                break;
            }
        });
        ReplicateStage{thread_hdl}
    }
}
