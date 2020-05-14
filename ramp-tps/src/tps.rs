use log::*;
use solana_client::perf_utils::{sample_txs, SampleStats};
use solana_client::thin_client::ThinClient;
use solana_notifier::Notifier;
use solana_sdk::timing::duration_as_s;
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    thread::{Builder, JoinHandle},
};

pub struct Sampler {
    client: Arc<ThinClient>,
    exit_signal: Arc<AtomicBool>,
    maxes: Arc<RwLock<Vec<(String, SampleStats)>>>,
    handle: Option<JoinHandle<()>>,
}

impl Sampler {
    pub fn new(rpc_addr: &SocketAddr) -> Self {
        let (_, dummy_socket) =
            solana_net_utils::bind_in_range(rpc_addr.ip(), (8000, 10_000)).unwrap();
        let dummy_tpu_addr = *rpc_addr;
        let client = Arc::new(ThinClient::new(*rpc_addr, dummy_tpu_addr, dummy_socket));

        Self {
            client,
            exit_signal: Arc::new(AtomicBool::new(false)),
            maxes: Arc::new(RwLock::new(Vec::new())),
            handle: None,
        }
    }

    // Setup a thread to sample every period and
    // collect the max transaction rate and total tx count seen
    pub fn start_sampling_thread(&mut self) {
        // Reset
        self.exit_signal.store(false, Ordering::Relaxed);
        self.maxes.write().unwrap().clear();

        let sample_period = 5; // in seconds
        info!("Sampling TPS every {} seconds...", sample_period);
        let exit_signal = self.exit_signal.clone();
        let maxes = self.maxes.clone();
        let client = self.client.clone();
        let handle = Builder::new()
            .name("solana-client-sample".to_string())
            .spawn(move || {
                sample_txs(&exit_signal, &maxes, sample_period, &client);
            })
            .unwrap();

        self.handle = Some(handle);
    }

    pub fn stop_sampling_thread(&mut self) {
        self.exit_signal.store(true, Ordering::Relaxed);
        self.handle.take().unwrap().join().unwrap();
    }

    pub fn report_results(&self, notifier: &Notifier) {
        let SampleStats { tps, elapsed, txs } = self.maxes.read().unwrap()[0].1;
        let avg_tps = txs as f32 / duration_as_s(&elapsed);
        notifier.send(&format!(
            "Highest TPS: {:.0}, Average TPS: {:.0}",
            tps, avg_tps
        ));
    }
}
