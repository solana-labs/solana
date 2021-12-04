use {
    solana_ledger::{blockstore::Blockstore, blockstore_meta::PerfSample},
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
    pub num_slots: u64,
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
        let forks = bank_forks.read().unwrap();
        let bank = forks.root_bank();
        let highest_slot = forks.highest_slot();
        drop(forks);

        let mut sample_snapshot = SamplePerformanceSnapshot {
            num_transactions: bank.transaction_count(),
            num_slots: highest_slot,
        };

        let mut now = Instant::now();
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let elapsed = now.elapsed();

            if elapsed.as_secs() >= SAMPLE_INTERVAL {
                now = Instant::now();
                let bank_forks = bank_forks.read().unwrap();
                let bank = bank_forks.root_bank().clone();
                let highest_slot = bank_forks.highest_slot();
                drop(bank_forks);

                let perf_sample = PerfSample {
                    num_slots: highest_slot
                        .checked_sub(sample_snapshot.num_slots)
                        .unwrap_or_default(),
                    num_transactions: bank
                        .transaction_count()
                        .checked_sub(sample_snapshot.num_transactions)
                        .unwrap_or_default(),
                    sample_period_secs: elapsed.as_secs() as u16,
                };

                if let Err(e) = blockstore.write_perf_sample(highest_slot, &perf_sample) {
                    error!("write_perf_sample failed: slot {:?} {:?}", highest_slot, e);
                }

                sample_snapshot = SamplePerformanceSnapshot {
                    num_transactions: bank.transaction_count(),
                    num_slots: highest_slot,
                };
            }

            sleep(Duration::from_millis(SLEEP_INTERVAL));
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
