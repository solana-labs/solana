//! The `poh_service` module implements a service that records the passing of
//! "ticks", a measure of time in the PoH stream

use crate::poh_recorder::{PohRecorder, PohRecorderError, WorkingLeader};
use crate::result::{Error, Result};
use crate::service::Service;
use solana_sdk::timing::NUM_TICKS_PER_SECOND;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

#[derive(Copy, Clone)]
pub enum PohServiceConfig {
    /// * `Tick` - Run full PoH thread.  Tick is a rough estimate of how many hashes to roll before
    ///            transmitting a new entry.
    Tick(usize),
    /// * `Sleep`- Low power mode.  Sleep is a rough estimate of how long to sleep before rolling 1
    ///            PoH once and producing 1 tick.
    Sleep(Duration),
}

impl Default for PohServiceConfig {
    fn default() -> PohServiceConfig {
        // TODO: Change this to Tick to enable PoH
        PohServiceConfig::Sleep(Duration::from_millis(1000 / NUM_TICKS_PER_SECOND as u64))
    }
}

pub struct PohService {
    tick_producer: JoinHandle<Result<()>>,
    poh_exit: Arc<AtomicBool>,
}

impl PohService {
    pub fn exit(&self) {
        self.poh_exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<Result<()>> {
        self.exit();
        self.join()
    }

    pub fn new(
        poh_recorder: PohRecorder,
        config: PohServiceConfig,
        poh_exit: Arc<AtomicBool>,
    ) -> (Self, Sender<WorkingLeader>) {
        // PohService is a headless producer, so when it exits it should notify the banking stage.
        // Since channel are not used to talk between these threads an AtomicBool is used as a
        // signal.
        let poh_exit_ = poh_exit.clone();
        let (leader_sender, leader_receiver) = channel();
        // Single thread to generate ticks
        let tick_producer = Builder::new()
            .name("solana-poh-service-tick_producer".to_string())
            .spawn(move || {
                let mut poh_recorder = poh_recorder;
                let leader_receiver = leader_receiver;
                let return_value =
                    Self::tick_producer(&leader_receiver, &mut poh_recorder, config, &poh_exit_);
                poh_exit_.store(true, Ordering::Relaxed);
                return_value
            })
            .unwrap();

        (
            Self {
                tick_producer,
                poh_exit,
            },
            leader_sender,
        )
    }

    fn tick_producer(
        leader_receiver: &Receiver<WorkingLeader>,
        poh: &mut PohRecorder,
        config: PohServiceConfig,
        poh_exit: &AtomicBool,
    ) -> Result<()> {
        let mut leader = None;
        loop {
            if leader.is_none() {
                let e = leader_receiver.try_recv();
                leader = match e {
                    Err(TryRecvError::Empty) => None,
                    _ => Some(e?),
                };
            }
            match config {
                PohServiceConfig::Tick(num) => {
                    for _ in 1..num {
                        poh.hash();
                    }
                }
                PohServiceConfig::Sleep(duration) => {
                    sleep(duration);
                }
            }
            let e = if let Some(ref current_leader) = leader {
                println!("tick");
                poh.tick(current_leader)
            } else {
                Ok(())
            };
            match e {
                Err(Error::PohRecorderError(PohRecorderError::MinHeightNotReached)) => (),
                Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached)) => leader = None,
                e => e?,
            };
            if poh_exit.load(Ordering::Relaxed) {
                return Ok(());
            }
        }
    }
}

impl Service for PohService {
    type JoinReturnType = Result<()>;

    fn join(self) -> thread::Result<Result<()>> {
        self.tick_producer.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tx::test_tx;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::hash;
    use std::sync::mpsc::channel;

    #[test]
    fn test_poh_service() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_id = bank.last_id();
        let (entry_sender, entry_receiver) = channel();
        let poh_recorder = PohRecorder::new(bank.tick_height(), prev_id);
        let exit = Arc::new(AtomicBool::new(false));
        let working_leader = WorkingLeader {
            bank: bank.clone(),
            sender: entry_sender,
            min_tick_height: bank.tick_height(),
            max_tick_height: std::u64::MAX,
        };

        let entry_producer: JoinHandle<Result<()>> = {
            let poh_recorder = poh_recorder.clone();
            let working_leader = working_leader.clone();
            let exit = exit.clone();

            Builder::new()
                .name("solana-poh-service-entry_producer".to_string())
                .spawn(move || {
                    loop {
                        // send some data
                        let h1 = hash(b"hello world!");
                        let tx = test_tx();
                        poh_recorder.record(h1, vec![tx], &working_leader).unwrap();

                        if exit.load(Ordering::Relaxed) {
                            break Ok(());
                        }
                    }
                })
                .unwrap()
        };

        const HASHES_PER_TICK: u64 = 2;
        let (poh_service, leader_sender) = PohService::new(
            poh_recorder.clone(),
            PohServiceConfig::Tick(HASHES_PER_TICK as usize),
            Arc::new(AtomicBool::new(false)),
        );

        leader_sender.send(working_leader.clone()).expect("send");

        // get some events
        let mut hashes = 0;
        let mut need_tick = true;
        let mut need_entry = true;
        let mut need_partial = true;

        while need_tick || need_entry || need_partial {
            for entry in entry_receiver.recv().unwrap() {
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
        poh_service.exit();
        let _ = poh_service.join().unwrap();
        let _ = entry_producer.join().unwrap();
    }
    #[test]
    fn test_poh_service_height() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_id = bank.last_id();
        let (entry_sender, entry_receiver) = channel();
        let poh_recorder = PohRecorder::new(bank.tick_height(), prev_id);
        let exit = Arc::new(AtomicBool::new(false));
        let working_leader = WorkingLeader {
            bank: bank.clone(),
            sender: entry_sender,
            min_tick_height: bank.tick_height() + 3,
            max_tick_height: bank.tick_height() + 5,
        };

        const HASHES_PER_TICK: u64 = 2;
        let (poh_service, leader_sender) = PohService::new(
            poh_recorder.clone(),
            PohServiceConfig::Tick(HASHES_PER_TICK as usize),
            Arc::new(AtomicBool::new(false)),
        );

        leader_sender.send(working_leader.clone()).expect("send");

        // all 5 ticks are expected
        let _ = entry_receiver.recv().unwrap();
        println!("x");
        let _ = entry_receiver.recv().unwrap();
        println!("x");
        let _ = entry_receiver.recv().unwrap();
        println!("x");
        let _ = entry_receiver.recv().unwrap();
        println!("x");
        let _ = entry_receiver.recv().unwrap();
        println!("x");

        //empty
        assert!(entry_receiver.try_recv().is_err());

        exit.store(true, Ordering::Relaxed);
        poh_service.exit();
        let _ = poh_service.join().unwrap();
    }
}
