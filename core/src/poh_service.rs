//! The `poh_service` module implements a service that records the passing of
//! "ticks", a measure of time in the PoH stream
use crate::poh_recorder::PohRecorder;
use crate::service::Service;
use solana_sdk::timing::{self, NUM_TICKS_PER_SECOND};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum PohServiceConfig {
    /// * `Tick` - Run full PoH thread.  Tick is a rough estimate of how many hashes to roll before
    ///            transmitting a new entry.
    Tick(usize),
    /// * `Sleep`- Low power mode.  Sleep is a rough estimate of how long to sleep before rolling 1
    ///            PoH once and producing 1 tick.
    Sleep(Duration),
    /// each node in simulation will be blocked until the receiver reads their step
    Step(SyncSender<()>),
}

impl Default for PohServiceConfig {
    fn default() -> PohServiceConfig {
        // TODO: Change this to Tick to enable PoH
        PohServiceConfig::Sleep(Duration::from_millis(1000 / NUM_TICKS_PER_SECOND))
    }
}

impl PohServiceConfig {
    pub fn ticks_to_ms(&self, num_ticks: u64) -> u64 {
        match self {
            PohServiceConfig::Sleep(d) => timing::duration_as_ms(d) * num_ticks,
            _ => panic!("Unsuppported tick config"),
        }
    }
}

pub struct PohService {
    tick_producer: JoinHandle<()>,
}

impl PohService {
    pub fn new(
        poh_recorder: Arc<Mutex<PohRecorder>>,
        config: &PohServiceConfig,
        poh_exit: &Arc<AtomicBool>,
    ) -> Self {
        // PohService is a headless producer, so when it exits it should notify the banking stage.
        // Since channel are not used to talk between these threads an AtomicBool is used as a
        // signal.
        let poh_exit_ = poh_exit.clone();
        // Single thread to generate ticks
        let config = config.clone();
        let tick_producer = Builder::new()
            .name("solana-poh-service-tick_producer".to_string())
            .spawn(move || {
                let poh_recorder = poh_recorder;
                Self::tick_producer(&poh_recorder, &config, &poh_exit_);
                poh_exit_.store(true, Ordering::Relaxed);
            })
            .unwrap();

        Self { tick_producer }
    }

    fn tick_producer(
        poh: &Arc<Mutex<PohRecorder>>,
        config: &PohServiceConfig,
        poh_exit: &AtomicBool,
    ) {
        loop {
            match config {
                PohServiceConfig::Tick(num) => {
                    for _ in 1..*num {
                        poh.lock().unwrap().hash();
                    }
                }
                PohServiceConfig::Sleep(duration) => {
                    sleep(*duration);
                }
                PohServiceConfig::Step(sender) => {
                    let r = sender.send(());
                    if r.is_err() {
                        break;
                    }
                }
            }
            poh.lock().unwrap().tick();
            if poh_exit.load(Ordering::Relaxed) {
                return;
            }
        }
    }
}

impl Service for PohService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.tick_producer.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::{get_tmp_ledger_path, Blocktree};
    use crate::leader_schedule_cache::LeaderScheduleCache;
    use crate::poh_recorder::WorkingBank;
    use crate::result::Result;
    use crate::test_tx::test_tx;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::hash;
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_poh_service() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_hash = bank.last_blockhash();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (poh_recorder, entry_receiver) = PohRecorder::new(
                bank.tick_height(),
                prev_hash,
                bank.slot(),
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );
            let poh_recorder = Arc::new(Mutex::new(poh_recorder));
            let exit = Arc::new(AtomicBool::new(false));
            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: bank.tick_height(),
                max_tick_height: std::u64::MAX,
            };

            let entry_producer: JoinHandle<Result<()>> = {
                let poh_recorder = poh_recorder.clone();
                let exit = exit.clone();

                Builder::new()
                    .name("solana-poh-service-entry_producer".to_string())
                    .spawn(move || {
                        loop {
                            // send some data
                            let h1 = hash(b"hello world!");
                            let tx = test_tx();
                            poh_recorder
                                .lock()
                                .unwrap()
                                .record(bank.slot(), h1, vec![tx])
                                .unwrap();

                            if exit.load(Ordering::Relaxed) {
                                break Ok(());
                            }
                        }
                    })
                    .unwrap()
            };

            const HASHES_PER_TICK: u64 = 2;
            let poh_service = PohService::new(
                poh_recorder.clone(),
                &PohServiceConfig::Tick(HASHES_PER_TICK as usize),
                &exit,
            );
            poh_recorder.lock().unwrap().set_working_bank(working_bank);

            // get some events
            let mut hashes = 0;
            let mut need_tick = true;
            let mut need_entry = true;
            let mut need_partial = true;

            while need_tick || need_entry || need_partial {
                for entry in entry_receiver.recv().unwrap().1 {
                    let entry = &entry.0;
                    if entry.is_tick() {
                        assert!(entry.num_hashes <= HASHES_PER_TICK);

                        if entry.num_hashes == HASHES_PER_TICK {
                            need_tick = false;
                        } else {
                            need_partial = false;
                        }

                        hashes += entry.num_hashes;

                        assert_eq!(hashes, HASHES_PER_TICK);

                        hashes = 0;
                    } else {
                        assert!(entry.num_hashes >= 1);
                        need_entry = false;
                        hashes += entry.num_hashes - 1;
                    }
                }
            }
            exit.store(true, Ordering::Relaxed);
            let _ = poh_service.join().unwrap();
            let _ = entry_producer.join().unwrap();
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }
}
