use solana_ledger::blockstore::Blockstore;
use solana_ledger::blockstore_meta::PerfSample;
use solana_runtime::bank_forks::BankForks;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    thread::{self, sleep, Builder, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const SAMPLE_INTERVAL: u64 = 60;
const SLEEP_INTERVAL: u64 = 500;

pub struct SamplePerformanceDelta {
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
        let mut sample_deltas: Option<SamplePerformanceDelta> = None;

        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let start = SystemTime::now();
            let since_epoch = start
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_millis() as u64;
            let elapsed_offset = since_epoch % (SAMPLE_INTERVAL * 1000);

            if elapsed_offset < SLEEP_INTERVAL {
                let bank_forks = bank_forks.read().unwrap();
                let bank = bank_forks.working_bank();
                let highest_slot = bank_forks.highest_slot();
                drop(bank_forks);

                match sample_deltas {
                    None => info!("Initializing SamplePerformance service"),
                    Some(ref snapshot) => {
                        let perf_sample = PerfSample {
                            num_slots: highest_slot - snapshot.num_slots,
                            num_transactions: bank.transaction_count() - snapshot.num_transactions,
                            sample_period_secs: SAMPLE_INTERVAL as u16,
                        };

                        if let Err(e) = blockstore.write_perf_sample(highest_slot, &perf_sample) {
                            error!("write_perf_sample failed: slot {:?} {:?}", highest_slot, e);
                        }
                    }
                }

                sample_deltas = Some(SamplePerformanceDelta {
                    num_transactions: bank.transaction_count(),
                    num_slots: highest_slot,
                });

                sleep(Duration::from_millis(SLEEP_INTERVAL - elapsed_offset));
            } else {
                sleep(Duration::from_millis(SLEEP_INTERVAL));
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
