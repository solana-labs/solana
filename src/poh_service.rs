//! The `poh_service` module implements a service that records the passing of
//! "ticks", a measure of time in the PoH stream

use poh_recorder::PohRecorder;
use result::Result;
use service::Service;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
pub const NUM_TICKS_PER_SECOND: usize = 10;

#[derive(Copy, Clone)]
pub enum Config {
    /// * `Tick` - Run full PoH thread.  Tick is a rough estimate of how many hashes to roll before transmitting a new entry.
    Tick(usize),
    /// * `Sleep`- Low power mode.  Sleep is a rough estimate of how long to sleep before rolling 1 poh once and producing 1
    /// tick.
    Sleep(Duration),
}

impl Default for Config {
    fn default() -> Config {
        // TODO: Change this to Tick to enable PoH
        Config::Sleep(Duration::from_millis(1000 / NUM_TICKS_PER_SECOND as u64))
    }
}

pub struct PohService {
    tick_producer: JoinHandle<Result<()>>,
    pub poh_exit: Arc<AtomicBool>,
}

impl PohService {
    pub fn exit(&self) -> () {
        self.poh_exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<Result<()>> {
        self.exit();
        self.join()
    }

    pub fn new(poh_recorder: PohRecorder, config: Config) -> Self {
        // PohService is a headless producer, so when it exits it should notify the banking stage.
        // Since channel are not used to talk between these threads an AtomicBool is used as a
        // signal.
        let poh_exit = Arc::new(AtomicBool::new(false));
        let poh_exit_ = poh_exit.clone();
        // Single thread to generate ticks
        let tick_producer = Builder::new()
            .name("solana-poh-service-tick_producer".to_string())
            .spawn(move || {
                let mut poh_recorder_ = poh_recorder;
                let return_value = Self::tick_producer(&mut poh_recorder_, config, &poh_exit_);
                poh_exit_.store(true, Ordering::Relaxed);
                return_value
            })
            .unwrap();

        PohService {
            tick_producer,
            poh_exit,
        }
    }

    fn tick_producer(poh: &mut PohRecorder, config: Config, poh_exit: &AtomicBool) -> Result<()> {
        loop {
            match config {
                Config::Tick(num) => {
                    for _ in 0..num {
                        poh.hash()?;
                    }
                }
                Config::Sleep(duration) => {
                    sleep(duration);
                }
            }
            poh.tick()?;
            if poh_exit.load(Ordering::Relaxed) {
                debug!("tick service exited");
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
