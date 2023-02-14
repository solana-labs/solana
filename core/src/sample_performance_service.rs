use {
    solana_ledger::{blockstore::Blockstore, blockstore_meta::PerfSampleV2},
    solana_runtime::bank_forks::BankForks,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const SAMPLE_INTERVAL: u64 = 60;
const SLEEP_INTERVAL: u64 = 500;

pub struct SamplePerformanceSnapshot {
    pub num_transactions: u64,
    pub num_non_vote_transactions: u64,
    pub highest_slot: u64,
}

pub struct SamplePerformanceService {
    thread_hdl: JoinHandle<()>,
}

impl SamplePerformanceService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        bank_forks: &Arc<RwLock<BankForks>>,
        blockstore: &Arc<Blockstore>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();
        let blockstore = blockstore.clone();
        let bank_forks = bank_forks.clone();

        info!("Starting SamplePerformance service");
        let thread_hdl = Builder::new()
            .name("sample-performance".to_string())
            .spawn(move || {
                Self::run(bank_forks, &blockstore, exit);
            })
            .unwrap();

        Self { thread_hdl }
    }

    pub fn run(
        bank_forks: Arc<RwLock<BankForks>>,
        blockstore: &Arc<Blockstore>,
        exit: Arc<AtomicBool>,
    ) {
        let mut snapshot = {
            let forks = bank_forks.read().unwrap();
            let bank = forks.root_bank();
            let highest_slot = forks.highest_slot();

            // Store the absolute transaction counts to that we can compute the
            // difference between these values at points in time to figure out
            // how many transactions occurred in that timespan.
            SamplePerformanceSnapshot {
                num_transactions: bank.transaction_count(),
                num_non_vote_transactions: bank.non_vote_transaction_count_since_restart(),
                highest_slot,
            }
        };

        let mut now = Instant::now();
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let elapsed = now.elapsed();

            if elapsed.as_secs() >= SAMPLE_INTERVAL {
                now = Instant::now();
                let (bank, highest_slot) = {
                    let bank_forks = bank_forks.read().unwrap();
                    (bank_forks.root_bank(), bank_forks.highest_slot())
                };

                let num_slots = highest_slot.saturating_sub(snapshot.highest_slot);
                let num_transactions = bank
                    .transaction_count()
                    .saturating_sub(snapshot.num_transactions);
                let num_non_vote_transactions = bank
                    .non_vote_transaction_count_since_restart()
                    .saturating_sub(snapshot.num_non_vote_transactions);

                let perf_sample = PerfSampleV2 {
                    num_slots,
                    num_transactions,
                    num_non_vote_transactions,
                    sample_period_secs: elapsed.as_secs() as u16,
                };

                if let Err(e) = blockstore.write_perf_sample(highest_slot, &perf_sample) {
                    error!("write_perf_sample failed: slot {:?} {:?}", highest_slot, e);
                }

                // Same as above, store the absolute transaction counts to use
                // as comparison for the next iteration of this loop.
                snapshot = SamplePerformanceSnapshot {
                    num_transactions: bank.transaction_count(),
                    num_non_vote_transactions: bank.non_vote_transaction_count_since_restart(),
                    highest_slot,
                };
            }

            sleep(Duration::from_millis(SLEEP_INTERVAL));
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
