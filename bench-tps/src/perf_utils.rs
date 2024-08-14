use {
    log::*,
    solana_sdk::commitment_config::CommitmentConfig,
    solana_tps_client::TpsClient,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::sleep,
        time::{Duration, Instant},
    },
};

#[derive(Default)]
pub struct SampleStats {
    /// Maximum TPS reported by this node
    pub tps: f32,
    /// Total transactions reported by this node
    pub txs: u64,
}

pub fn sample_txs<T>(
    exit_signal: Arc<AtomicBool>,
    sample_stats: &Arc<RwLock<Vec<(String, SampleStats)>>>,
    sample_period: u64,
    client: &Arc<T>,
) where
    T: TpsClient + ?Sized,
{
    let mut max_tps = 0.0;
    let mut total_elapsed;
    let mut total_txs;
    let mut now = Instant::now();
    let start_time = now;
    let initial_txs = client
        .get_transaction_count_with_commitment(CommitmentConfig::processed())
        .expect("transaction count");
    let mut last_txs = initial_txs;

    loop {
        total_elapsed = start_time.elapsed();
        let elapsed = now.elapsed();
        now = Instant::now();
        let mut txs =
            match client.get_transaction_count_with_commitment(CommitmentConfig::processed()) {
                Err(e) => {
                    info!("Couldn't get transaction count {:?}", e);
                    sleep(Duration::from_secs(sample_period));
                    continue;
                }
                Ok(tx_count) => tx_count,
            };

        if txs < last_txs {
            info!("Expected txs({}) >= last_txs({})", txs, last_txs);
            txs = last_txs;
        }
        total_txs = txs - initial_txs;
        let sample_txs = txs - last_txs;
        last_txs = txs;

        let tps = sample_txs as f32 / elapsed.as_secs_f32();
        if tps > max_tps {
            max_tps = tps;
        }

        info!(
            "Sampler {:9.2} TPS, Transactions: {:6}, Total transactions: {} over {} s",
            tps,
            sample_txs,
            total_txs,
            total_elapsed.as_secs(),
        );

        if exit_signal.load(Ordering::Relaxed) {
            let stats = SampleStats {
                tps: max_tps,
                txs: total_txs,
            };
            sample_stats.write().unwrap().push((client.addr(), stats));
            return;
        }
        sleep(Duration::from_secs(sample_period));
    }
}
