#![allow(clippy::arithmetic_side_effects)]
use {
    log::*,
    solana_measure::measure::Measure,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        commitment_config::CommitmentConfig, signature::Signature, timing::timestamp,
        transaction::Transaction,
    },
    std::{
        net::SocketAddr,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, RwLock,
        },
        thread::{sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

// signature, timestamp, id
type PendingQueue = Vec<(Signature, u64, u64)>;

pub struct TransactionExecutor {
    sig_clear_t: JoinHandle<()>,
    sigs: Arc<RwLock<PendingQueue>>,
    cleared: Arc<RwLock<Vec<u64>>>,
    exit: Arc<AtomicBool>,
    counter: AtomicU64,
    client: Arc<RpcClient>,
}

impl TransactionExecutor {
    pub fn new(entrypoint_addr: SocketAddr) -> Self {
        let client = Arc::new(RpcClient::new_socket_with_commitment(
            entrypoint_addr,
            CommitmentConfig::confirmed(),
        ));
        Self::new_with_rpc_client(client)
    }

    pub fn new_with_url<U: ToString>(url: U) -> Self {
        let client = Arc::new(RpcClient::new_with_commitment(
            url,
            CommitmentConfig::confirmed(),
        ));
        Self::new_with_rpc_client(client)
    }

    pub fn new_with_rpc_client(client: Arc<RpcClient>) -> Self {
        let sigs = Arc::new(RwLock::new(Vec::new()));
        let cleared = Arc::new(RwLock::new(Vec::new()));
        let exit = Arc::new(AtomicBool::new(false));
        let sig_clear_t = Self::start_sig_clear_thread(exit.clone(), &sigs, &cleared, &client);
        Self {
            sigs,
            cleared,
            sig_clear_t,
            exit,
            counter: AtomicU64::new(0),
            client,
        }
    }

    pub fn num_outstanding(&self) -> usize {
        self.sigs.read().unwrap().len()
    }

    pub fn push_transactions(&self, txs: Vec<Transaction>) -> Vec<u64> {
        let mut ids = vec![];
        let new_sigs = txs.into_iter().filter_map(|tx| {
            let id = self.counter.fetch_add(1, Ordering::Relaxed);
            ids.push(id);
            match self.client.send_transaction(&tx) {
                Ok(sig) => {
                    return Some((sig, timestamp(), id));
                }
                Err(e) => {
                    info!("error: {:#?}", e);
                }
            }
            None
        });
        let mut sigs_w = self.sigs.write().unwrap();
        sigs_w.extend(new_sigs);
        ids
    }

    pub fn drain_cleared(&self) -> Vec<u64> {
        std::mem::take(&mut *self.cleared.write().unwrap())
    }

    pub fn close(self) {
        self.exit.store(true, Ordering::Relaxed);
        self.sig_clear_t.join().unwrap();
    }

    fn start_sig_clear_thread(
        exit: Arc<AtomicBool>,
        sigs: &Arc<RwLock<PendingQueue>>,
        cleared: &Arc<RwLock<Vec<u64>>>,
        client: &Arc<RpcClient>,
    ) -> JoinHandle<()> {
        let sigs = sigs.clone();
        let cleared = cleared.clone();
        let client = client.clone();
        Builder::new()
            .name("solSigClear".to_string())
            .spawn(move || {
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
                                let only_sigs: Vec<_> = sig_chunk.iter().map(|s| s.0).collect();
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
                            let sent_ts = sigs_w[i].1;
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
                                new_ids.push(sigs_w.remove(i).2);
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
