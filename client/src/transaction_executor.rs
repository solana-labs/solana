#![allow(clippy::integer_arithmetic)]
use crate::{
    rpc_client::RpcClient,
    rpc_config::RpcSendTransactionConfig,
    tpu_client::{TpuClient, TpuClientConfig},
};
use log::*;
use solana_measure::measure::Measure;
use solana_sdk::{
    commitment_config::CommitmentConfig, signature::Signature, timing::timestamp,
    transaction::Transaction,
};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, RwLock,
    },
    thread::{sleep, Builder, JoinHandle},
    time::{Duration, Instant},
};

type PendingSendQueue = VecDeque<Vec<(u64, Transaction)>>;

struct SentQueueEntry {
    sig: Signature,
    sent_ts: u64,
    id: u64,
}
type SentQueue = Vec<SentQueueEntry>;

pub struct TransactionExecutor {
    sig_clear_t: JoinHandle<()>,
    transfer_t: JoinHandle<()>,

    pending_transactions: Arc<RwLock<PendingSendQueue>>,

    sigs: Arc<RwLock<SentQueue>>,
    cleared: Arc<RwLock<Vec<u64>>>,

    exit: Arc<AtomicBool>,
    counter: AtomicU64,
}

impl TransactionExecutor {
    pub fn new(
        entrypoint_addr: SocketAddr,
        skip_preflight: bool,
        use_tpu_client: bool,
        tpu_client_fanout_slots: Option<u64>,
    ) -> Self {
        let sigs = Arc::new(RwLock::new(Vec::new()));
        let cleared = Arc::new(RwLock::new(Vec::new()));
        let exit = Arc::new(AtomicBool::new(false));
        let pending_transactions = Arc::new(RwLock::new(VecDeque::new()));
        let sig_clear_t = Self::start_sig_clear_thread(&exit, &sigs, &cleared, entrypoint_addr);
        let transfer_t = Self::start_transfer_thread(
            &exit,
            &sigs,
            &pending_transactions,
            skip_preflight,
            &cleared,
            entrypoint_addr,
            use_tpu_client,
            tpu_client_fanout_slots,
        );
        Self {
            sigs,
            cleared,
            sig_clear_t,
            exit,
            counter: AtomicU64::new(0),
            pending_transactions,
            transfer_t,
        }
    }

    pub fn num_outstanding(&self) -> usize {
        self.sigs.read().unwrap().len()
    }

    fn start_transfer_thread(
        exit: &Arc<AtomicBool>,
        sigs: &Arc<RwLock<SentQueue>>,
        pending_transactions: &Arc<RwLock<PendingSendQueue>>,
        skip_preflight: bool,
        cleared: &Arc<RwLock<Vec<u64>>>,
        entrypoint_addr: SocketAddr,
        use_tpu_client: bool,
        tpu_client_fanout_slots: Option<u64>,
    ) -> JoinHandle<()> {
        let exit = exit.clone();
        let sigs = sigs.clone();
        let pending_transactions = pending_transactions.clone();
        let cleared = cleared.clone();
        Builder::new()
            .name("transfer".to_string())
            .spawn(move || {
                let client = Arc::new(RpcClient::new_socket_with_commitment(
                    entrypoint_addr,
                    CommitmentConfig::confirmed(),
                ));
                let tpu_client = if use_tpu_client {
                    let mut tpu_config = TpuClientConfig::default();
                    if let Some(fanout_slots) = tpu_client_fanout_slots {
                        tpu_config.fanout_slots = fanout_slots;
                    }
                    Some(TpuClient::new(client.clone(), "", tpu_config).unwrap())
                } else {
                    None
                };

                while !exit.load(Ordering::Relaxed) {
                    let txs = {
                        let mut pending_transactions = pending_transactions.write().unwrap();
                        pending_transactions.pop_front()
                    };
                    if let Some(txs) = txs {
                        let mut errored_ids = Vec::new();
                        let new_sigs = txs.into_iter().filter_map(|(id, tx)| {
                            match tpu_client.as_ref() {
                                Some(tpu_client) => {
                                    if tpu_client.send_transaction(&tx) {
                                        return Some(SentQueueEntry {
                                            sig: tx.signatures[0],
                                            sent_ts: timestamp(),
                                            id,
                                        });
                                    }
                                }
                                None => {
                                    let config = RpcSendTransactionConfig {
                                        skip_preflight,
                                        ..RpcSendTransactionConfig::default()
                                    };
                                    match client.send_transaction_with_config(&tx, config) {
                                        Ok(sig) => {
                                            return Some(SentQueueEntry {
                                                sig,
                                                sent_ts: timestamp(),
                                                id,
                                            });
                                        }
                                        Err(e) => {
                                            info!("error: {:#?}", e);
                                            errored_ids.push(id);
                                        }
                                    }
                                }
                            }
                            None
                        });
                        {
                            let mut sigs_w = sigs.write().unwrap();
                            sigs_w.extend(new_sigs);
                        }
                        {
                            let mut cleared = cleared.write().unwrap();
                            cleared.extend(errored_ids);
                        }
                    } else {
                        sleep(Duration::from_millis(10));
                    }
                }
            })
            .unwrap()
    }

    pub fn push_transactions(&self, txs: Vec<Transaction>) -> Vec<u64> {
        let txs_len = txs.len() as u64;
        let latest_id = self.counter.fetch_add(txs_len, Ordering::Relaxed);
        let new_ids: Vec<_> = (latest_id..latest_id + txs_len).into_iter().collect();
        assert_eq!(new_ids.len(), txs.len());
        let txs: Vec<_> = new_ids.clone().into_iter().zip(txs.into_iter()).collect();

        let mut pending_transactions = self.pending_transactions.write().unwrap();
        pending_transactions.push_back(txs);

        new_ids
    }

    pub fn drain_cleared(&self) -> Vec<u64> {
        std::mem::take(&mut *self.cleared.write().unwrap())
    }

    pub fn close(self) {
        self.exit.store(true, Ordering::Relaxed);
        self.sig_clear_t.join().unwrap();
        self.transfer_t.join().unwrap();
    }

    fn start_sig_clear_thread(
        exit: &Arc<AtomicBool>,
        sigs: &Arc<RwLock<SentQueue>>,
        cleared: &Arc<RwLock<Vec<u64>>>,
        entrypoint_addr: SocketAddr,
    ) -> JoinHandle<()> {
        let sigs = sigs.clone();
        let exit = exit.clone();
        let cleared = cleared.clone();
        Builder::new()
            .name("sig_clear".to_string())
            .spawn(move || {
                let client = RpcClient::new_socket_with_commitment(
                    entrypoint_addr,
                    CommitmentConfig::confirmed(),
                );
                let mut success = 0;
                let mut error_count = 0;
                let mut timed_out = 0;
                let mut last_log = Instant::now();
                while !exit.load(Ordering::Relaxed) {
                    let sigs_len = sigs.read().unwrap().len();
                    if sigs_len > 0 {
                        let mut sigs_w = sigs.write().unwrap();
                        let mut start = Measure::start("sig_status");
                        let statuses: Vec<_> = sigs_w
                            .chunks(200)
                            .flat_map(|sig_chunk| {
                                let only_sigs: Vec<_> = sig_chunk.iter().map(|s| s.sig).collect();
                                client
                                    .get_signature_statuses(&only_sigs)
                                    .expect("status fail")
                                    .value
                            })
                            .collect();
                        let mut num_cleared = 0;
                        let start_len = sigs_w.len();
                        let now = timestamp();
                        let mut new_ids = vec![];
                        let mut i = 0;
                        let mut j = 0;
                        while i != sigs_w.len() {
                            let mut retain = true;
                            let sent_ts = sigs_w[i].sent_ts;
                            if let Some(e) = &statuses[j] {
                                debug!("error: {:?}", e);
                                if e.status.is_ok() {
                                    success += 1;
                                } else {
                                    error_count += 1;
                                }
                                num_cleared += 1;
                                retain = false;
                            } else if now - sent_ts > 30_000 {
                                retain = false;
                                timed_out += 1;
                            }
                            if !retain {
                                new_ids.push(sigs_w.remove(i).id);
                            } else {
                                i += 1;
                            }
                            j += 1;
                        }
                        let final_sigs_len = sigs_w.len();
                        drop(sigs_w);
                        cleared.write().unwrap().extend(new_ids);
                        start.stop();
                        debug!(
                            "sigs len: {:?} success: {} took: {}ms cleared: {}/{}",
                            final_sigs_len,
                            success,
                            start.as_ms(),
                            num_cleared,
                            start_len,
                        );
                        if last_log.elapsed().as_millis() > 5000 {
                            info!(
                                "success: {} error: {} timed_out: {}",
                                success, error_count, timed_out,
                            );
                            last_log = Instant::now();
                        }
                    }
                    sleep(Duration::from_millis(200));
                }
            })
            .unwrap()
    }
}
