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

pub struct SamplePerformanceService {
    thread_hdl: JoinHandle<()>,
}

impl SamplePerformanceService {
    pub fn new(
        bank_forks: &Arc<RwLock<BankForks>>,
        blockstore: Arc<Blockstore>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let bank_forks = bank_forks.clone();

        info!("Starting SamplePerformance service");
        let thread_hdl = Builder::new()
            .name("sample-performance".to_string())
            .spawn(move || {
                Self::run(bank_forks, blockstore, exit);
            })
            .unwrap();

        Self { thread_hdl }
    }

    pub fn run(
        bank_forks: Arc<RwLock<BankForks>>,
        blockstore: Arc<Blockstore>,
        exit: Arc<AtomicBool>,
    ) {
        let mut snapshot = StatsSnapshot::from_forks(&bank_forks);

        let mut now = Instant::now();
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let elapsed = now.elapsed();

            if elapsed.as_secs() >= SAMPLE_INTERVAL {
                now = Instant::now();
                let new_snapshot = StatsSnapshot::from_forks(&bank_forks);

                let (num_transactions, num_non_vote_transactions, num_slots) =
                    new_snapshot.diff_since(&snapshot);

                // Store the new snapshot to compare against in the next iteration of the loop.
                snapshot = new_snapshot;

                let perf_sample = PerfSampleV2 {
                    // Note: since num_slots is computed from the highest slot and not the bank
                    // slot, this value should not be used in conjunction with num_transactions or
                    // num_non_vote_transactions to draw any conclusions about number of
                    // transactions per slot.
                    num_slots,
                    num_transactions,
                    num_non_vote_transactions,
                    sample_period_secs: elapsed.as_secs() as u16,
                };

                let highest_slot = snapshot.highest_slot;
                if let Err(e) = blockstore.write_perf_sample(highest_slot, &perf_sample) {
                    error!("write_perf_sample failed: slot {:?} {:?}", highest_slot, e);
                }
            }

            sleep(Duration::from_millis(SLEEP_INTERVAL));
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

struct StatsSnapshot {
    pub num_transactions: u64,
    pub num_non_vote_transactions: u64,
    pub highest_slot: u64,
}

impl StatsSnapshot {
    fn from_forks(forks: &RwLock<BankForks>) -> Self {
        let forks = forks.read().unwrap();
        let bank = forks.root_bank();
        Self {
            num_transactions: bank.transaction_count(),
            num_non_vote_transactions: bank.non_vote_transaction_count_since_restart(),
            highest_slot: forks.highest_slot(),
        }
    }

    fn diff_since(&self, predecessor: &Self) -> (u64, u64, u64) {
        (
            self.num_transactions
                .saturating_sub(predecessor.num_transactions),
            self.num_non_vote_transactions
                .saturating_sub(predecessor.num_non_vote_transactions),
            self.highest_slot.saturating_sub(predecessor.highest_slot),
        )
    }
}
