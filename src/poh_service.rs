//! The `poh_service` module implements a service that records the passing of
//! "ticks", a measure of time in the PoH stream

use crate::poh_recorder::PohRecorder;
use crate::service::Service;
use solana_sdk::timing::NUM_TICKS_PER_SECOND;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

#[derive(Clone)]
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

pub struct PohService {
    tick_producer: JoinHandle<()>,
    poh_exit: Arc<AtomicBool>,
}

impl PohService {
    pub fn exit(&self) {
        self.poh_exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
    }

    pub fn new(
        poh_recorder: Arc<Mutex<PohRecorder>>,
        config: &PohServiceConfig,
        poh_exit: Arc<AtomicBool>,
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

        Self {
            tick_producer,
            poh_exit,
        }
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

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::poh_recorder::WorkingBank;
//     use crate::result::Result;
//     use crate::test_tx::test_tx;
//     use solana_runtime::bank::Bank;
//     use solana_sdk::genesis_block::GenesisBlock;
//     use solana_sdk::hash::hash;
//     use std::sync::mpsc::channel;
//     use std::sync::mpsc::RecvError;
//
//     #[test]
//     fn test_poh_service() {
//         let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
//         let bank = Arc::new(Bank::new(&genesis_block));
//         let prev_hash = bank.last_id();
//         let (entry_sender, entry_receiver) = channel();
//         let poh_recorder = Arc::new(Mutex::new(PohRecorder::new(bank.tick_height(), prev_hash)));
//         let exit = Arc::new(AtomicBool::new(false));
//         let working_bank = WorkingBank {
//             bank: bank.clone(),
//             sender: entry_sender,
//             min_tick_height: bank.tick_height(),
//             max_tick_height: std::u64::MAX,
//         };
//
//         let entry_producer: JoinHandle<Result<()>> = {
//             let poh_recorder = poh_recorder.clone();
//             let exit = exit.clone();
//
//             Builder::new()
//                 .name("solana-poh-service-entry_producer".to_string())
//                 .spawn(move || {
//                     loop {
//                         // send some data
//                         let h1 = hash(b"hello world!");
//                         let tx = test_tx();
//                         poh_recorder.lock().unwrap().record(h1, vec![tx]).unwrap();
//
//                         if exit.load(Ordering::Relaxed) {
//                             break Ok(());
//                         }
//                     }
//                 })
//                 .unwrap()
//         };
//
//         const HASHES_PER_TICK: u64 = 2;
//         let poh_service = PohService::new(
//             poh_recorder.clone(),
//             &PohServiceConfig::Tick(HASHES_PER_TICK as usize),
//             Arc::new(AtomicBool::new(false)),
//         );
//         poh_recorder.lock().unwrap().set_working_bank(working_bank);
//
//         // get some events
//         let mut hashes = 0;
//         let mut need_tick = true;
//         let mut need_entry = true;
//         let mut need_partial = true;
//
//         while need_tick || need_entry || need_partial {
//             for entry in entry_receiver.recv().unwrap() {
//                 let entry = &entry.0;
//                 if entry.is_tick() {
//                     assert!(entry.num_hashes <= HASHES_PER_TICK);
//
//                     if entry.num_hashes == HASHES_PER_TICK {
//                         need_tick = false;
//                     } else {
//                         need_partial = false;
//                     }
//
//                     hashes += entry.num_hashes;
//
//                     assert_eq!(hashes, HASHES_PER_TICK);
//
//                     hashes = 0;
//                 } else {
//                     assert!(entry.num_hashes >= 1);
//                     need_entry = false;
//                     hashes += entry.num_hashes - 1;
//                 }
//             }
//         }
//         exit.store(true, Ordering::Relaxed);
//         poh_service.exit();
//         let _ = poh_service.join().unwrap();
//         let _ = entry_producer.join().unwrap();
//     }
//
//     #[test]
//     fn test_poh_service_drops_working_bank() {
//         let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
//         let bank = Arc::new(Bank::new(&genesis_block));
//         let prev_hash = bank.last_id();
//         let (entry_sender, entry_receiver) = channel();
//         let poh_recorder = Arc::new(Mutex::new(PohRecorder::new(bank.tick_height(), prev_hash)));
//         let exit = Arc::new(AtomicBool::new(false));
//         let working_bank = WorkingBank {
//             bank: bank.clone(),
//             sender: entry_sender,
//             min_tick_height: bank.tick_height() + 3,
//             max_tick_height: bank.tick_height() + 5,
//         };
//
//         let poh_service = PohService::new(
//             poh_recorder.clone(),
//             &PohServiceConfig::default(),
//             Arc::new(AtomicBool::new(false)),
//         );
//
//         poh_recorder.lock().unwrap().set_working_bank(working_bank);
//
//         // all 5 ticks are expected, there is no tick 0
//         // First 4 ticks must be sent all at once, since bank shouldn't see them until
//         // the after bank's min_tick_height(3) is reached.
//         let entries = entry_receiver.recv().expect("recv 1");
//         assert_eq!(entries.len(), 4);
//         let entries = entry_receiver.recv().expect("recv 2");
//         assert_eq!(entries.len(), 1);
//
//         //WorkingBank should be dropped by the PohService thread as well
//         assert_eq!(entry_receiver.recv(), Err(RecvError));
//
//         exit.store(true, Ordering::Relaxed);
//         poh_service.exit();
//         let _ = poh_service.join().unwrap();
//     }
// }
