//! The `replicate_stage` replicates transactions broadcast by the leader.

use bank::Bank;
use counter::Counter;
use crdt::Crdt;
use ledger::{reconstruct_entries_from_blobs, Block, LedgerWriter};
use log::Level;
use packet::BlobRecycler;
use result::{Error, Result};
use service::Service;
use signature::Keypair;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::channel;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use streamer::{responder, BlobReceiver};
use vote_stage::VoteStage;

pub struct ReplicateStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ReplicateStage {
    /// Process entry blobs, already in order
    fn replicate_requests(
        bank: &Arc<Bank>,
        crdt: &Arc<RwLock<Crdt>>,
        blob_recycler: &BlobRecycler,
        window_receiver: &BlobReceiver,
        ledger_writer: Option<&mut LedgerWriter>,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        //coalesce all the available blobs into a single vote
        let mut blobs = window_receiver.recv_timeout(timer)?;
        while let Ok(mut more) = window_receiver.try_recv() {
            blobs.append(&mut more);
        }
        let entries = reconstruct_entries_from_blobs(blobs.clone())?;

        let res = bank.process_entries(entries.clone());

        for blob in blobs {
            blob_recycler.recycle(blob, "replicate_requests");
        }

        {
            let mut wcrdt = crdt.write().unwrap();
            wcrdt.insert_votes(&entries.votes());
        }

        inc_new_counter_info!(
            "replicate-transactions",
            entries.iter().map(|x| x.transactions.len()).sum()
        );

        // TODO: move this to another stage?
        if let Some(ledger_writer) = ledger_writer {
            ledger_writer.write_entries(entries)?;
        }

        res?;
        Ok(())
    }
    pub fn new(
        keypair: Keypair,
        bank: Arc<Bank>,
        crdt: Arc<RwLock<Crdt>>,
        blob_recycler: BlobRecycler,
        window_receiver: BlobReceiver,
        ledger_path: Option<&str>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let (vote_blob_sender, vote_blob_receiver) = channel();
        let send = UdpSocket::bind("0.0.0.0:0").expect("bind");
        let t_responder = responder(
            "replicate_stage",
            Arc::new(send),
            blob_recycler.clone(),
            vote_blob_receiver,
        );

        let vote_stage = VoteStage::new(
            Arc::new(keypair),
            bank.clone(),
            crdt.clone(),
            blob_recycler.clone(),
            vote_blob_sender,
            exit,
        );

        let mut ledger_writer = ledger_path.map(|p| LedgerWriter::open(p, false).unwrap());

        let t_replicate = Builder::new()
            .name("solana-replicate-stage".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::replicate_requests(
                    &bank,
                    &crdt,
                    &blob_recycler,
                    &window_receiver,
                    ledger_writer.as_mut(),
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap();

        let mut thread_hdls = vec![t_responder, t_replicate];
        thread_hdls.extend(vote_stage.thread_hdls());

        ReplicateStage { thread_hdls }
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
