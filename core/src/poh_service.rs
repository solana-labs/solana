//! The `poh_service` module implements a service that records the passing of
//! "ticks", a measure of time in the PoH stream
use crate::poh_recorder::PohRecorder;
use crate::service::Service;
use solana_sdk::poh_config::PohConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Instant;

pub struct PohService {
    tick_producer: JoinHandle<()>,
}

impl PohService {
    pub fn new(
        poh_recorder: Arc<Mutex<PohRecorder>>,
        config: &PohConfig,
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
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        config: &PohConfig,
        poh_exit: &AtomicBool,
    ) {
        while !poh_exit.load(Ordering::Relaxed) {
            let tick_start = Instant::now();

            if config.hashes_per_tick == 0 {
                let hashing_duration = Instant::now().duration_since(tick_start);
                if hashing_duration < config.target_tick_duration {
                    sleep(config.target_tick_duration - hashing_duration);
                }
            } else {
                for _ in 1..config.hashes_per_tick {
                    // TODO: amortize the cost of this lock by doing the loop in here for
                    // some min amount of hashes
                    // TODO: account for any extra hashes added by `poh_recorder.record()
                    poh_recorder.lock().unwrap().hash(1);
                }
            }

            poh_recorder.lock().unwrap().tick();

            let tick_duration = Instant::now().duration_since(tick_start);
            // TODO: add datapoint!() for tick info
            info!(
                "tick: {} hashes in {}ms (drift {}ms)",
                config.hashes_per_tick + 1,
                tick_duration.as_millis(),
                config.target_tick_duration.as_millis() as i64 - tick_duration.as_millis() as i64
            );
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
    use crate::genesis_utils::create_genesis_block;
    use crate::leader_schedule_cache::LeaderScheduleCache;
    use crate::poh_recorder::WorkingBank;
    use crate::result::Result;
    use crate::test_tx::test_tx;
    use solana_runtime::bank::Bank;
    use solana_sdk::hash::hash;
    use solana_sdk::pubkey::Pubkey;
    use std::time::Duration;

    #[test]
    fn test_poh_service() {
        let (genesis_block, _mint_keypair) = create_genesis_block(2);
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

            let config = PohConfig {
                hashes_per_tick: 2,
                target_tick_duration: Duration::from_millis(1),
            };
            let poh_service = PohService::new(poh_recorder.clone(), &config, &exit);
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
                        assert!(
                            entry.num_hashes <= config.hashes_per_tick,
                            format!("{} <= {}", entry.num_hashes, config.hashes_per_tick)
                        );

                        if entry.num_hashes == config.hashes_per_tick {
                            need_tick = false;
                        } else {
                            need_partial = false;
                        }

                        hashes += entry.num_hashes;

                        assert_eq!(hashes, config.hashes_per_tick);

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
