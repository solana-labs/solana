//! The `replicate_stage` replicates transactions broadcast by the leader.

use bank::Bank;
use bincode::serialize;
use counter::Counter;
use crdt::Crdt;
use ledger;
use packet::BlobRecycler;
use result::{Error, Result};
use service::Service;
use signature::KeyPair;
use std::collections::VecDeque;
use std::net::UdpSocket;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::channel;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use streamer::{responder, BlobReceiver, BlobSender};
use timing;
use transaction::Transaction;
use voting::entries_to_votes;

pub struct ReplicateStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

const VOTE_TIMEOUT_MS: u64 = 1000;
const LOG_RATE: usize = 10;

impl ReplicateStage {
    /// Process entry blobs, already in order
    fn replicate_requests(
        keypair: &Arc<KeyPair>,
        bank: &Arc<Bank>,
        crdt: &Arc<RwLock<Crdt>>,
        blob_recycler: &BlobRecycler,
        window_receiver: &BlobReceiver,
        vote_blob_sender: &BlobSender,
        last_vote: &mut u64,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        //coalesce all the available blobs into a single vote
        let mut blobs = window_receiver.recv_timeout(timer)?;
        while let Ok(mut more) = window_receiver.try_recv() {
            blobs.append(&mut more);
        }
        let blobs_len = blobs.len();
        let entries = ledger::reconstruct_entries_from_blobs(blobs.clone())?;
        let votes = entries_to_votes(&entries);

        static mut COUNTER_REPLICATE: Counter = create_counter!("replicate-transactions", LOG_RATE);
        inc_counter!(
            COUNTER_REPLICATE,
            entries.iter().map(|x| x.transactions.len()).sum()
        );
        let res = bank.process_entries(entries);
        if res.is_err() {
            error!("process_entries {} {:?}", blobs_len, res);
        }
        let now = timing::timestamp();
        if now - *last_vote > VOTE_TIMEOUT_MS {
            let height = res?;
            let last_id = bank.last_id();
            let shared_blob = blob_recycler.allocate();
            let (vote, addr) = {
                let mut wcrdt = crdt.write().unwrap();
                wcrdt.insert_votes(votes);
                //TODO: doesn't seem like there is a synchronous call to get height and id
                info!("replicate_stage {} {:?}", height, &last_id[..8]);
                wcrdt.new_vote(height, last_id)
            }?;
            {
                let mut blob = shared_blob.write().unwrap();
                let tx = Transaction::new_vote(&keypair, vote, last_id, 0);
                let bytes = serialize(&tx)?;
                let len = bytes.len();
                blob.data[..len].copy_from_slice(&bytes);
                blob.meta.set_addr(&addr);
                blob.meta.size = len;
            }
            *last_vote = now;
            vote_blob_sender.send(VecDeque::from(vec![shared_blob]))?;
        }
        while let Some(blob) = blobs.pop_front() {
            blob_recycler.recycle(blob);
        }
        Ok(())
    }
    pub fn new(
        keypair: KeyPair,
        bank: Arc<Bank>,
        crdt: Arc<RwLock<Crdt>>,
        blob_recycler: BlobRecycler,
        window_receiver: BlobReceiver,
    ) -> Self {
        let (vote_blob_sender, vote_blob_receiver) = channel();
        let send = UdpSocket::bind("0.0.0.0:0").expect("bind");
        let t_responder = responder(send, blob_recycler.clone(), vote_blob_receiver);
        let skeypair = Arc::new(keypair);

        let t_replicate = Builder::new()
            .name("solana-replicate-stage".to_string())
            .spawn(move || {
                let mut timestamp: u64 = 0;
                loop {
                    if let Err(e) = Self::replicate_requests(
                        &skeypair,
                        &bank,
                        &crdt,
                        &blob_recycler,
                        &window_receiver,
                        &vote_blob_sender,
                        &mut timestamp,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            _ => error!("{:?}", e),
                        }
                    }
                }
            })
            .unwrap();
        ReplicateStage {
            thread_hdls: vec![t_responder, t_replicate],
        }
    }
}

impl Service for ReplicateStage {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        self.thread_hdls
    }
    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls() {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
