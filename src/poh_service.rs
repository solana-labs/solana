//! The `poh_service` module provides an object for generating a Proof of History.
//! It records Hashes items on behalf of its users. It continuously generates
//! new Hashes, only stopping to check if it has been sent a Hash to mix in
//! to the Poh.
//!
//! The returned Entry includes the mix-in request, the latest Poh Hash, and the
//! number of Hashes generated in the service since the last mix-in request.
//!
//! The resulting stream of Hashes represents ordered events in time.
//!
use hash::Hash;
use poh::{Poh, PohEntry};
use service::Service;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, Builder, JoinHandle};

pub struct PohService {
    poh: Arc<Mutex<Poh>>,
    thread_hdl: JoinHandle<()>,
    run_poh: Arc<AtomicBool>,
}

impl PohService {
    /// A background thread that will continue tagging received Transaction messages and
    /// sending back Entry messages until either the receiver or sender channel is closed.
    /// if tick_duration is some, service will automatically produce entries every
    ///  `tick_duration`.
    pub fn new(start_hash: Hash, run_poh: bool) -> Self {
        let poh = Arc::new(Mutex::new(Poh::new(start_hash)));
        let run_poh = Arc::new(AtomicBool::new(run_poh));

        let thread_poh = poh.clone();
        let thread_run_poh = run_poh.clone();
        let thread_hdl = Builder::new()
            .name("solana-record-service".to_string())
            .spawn(move || {
                while thread_run_poh.load(Ordering::Relaxed) {
                    thread_poh.lock().unwrap().hash();
                }
            }).unwrap();

        PohService {
            poh,
            run_poh,
            thread_hdl,
        }
    }

    pub fn tick(&self) -> PohEntry {
        let mut poh = self.poh.lock().unwrap();
        poh.tick()
    }

    pub fn record(&self, mixin: Hash) -> PohEntry {
        let mut poh = self.poh.lock().unwrap();
        poh.record(mixin)
    }

    //    fn process_hash(
    //        mixin: Option<Hash>,
    //        poh: &mut Poh,
    //        sender: &Sender<PohEntry>,
    //    ) -> Result<(), ()> {
    //        let resp = match mixin {
    //            Some(mixin) => poh.record(mixin),
    //            None => poh.tick(),
    //        };
    //        sender.send(resp).or(Err(()))?;
    //        Ok(())
    //    }
    //
    //    fn process_hashes(
    //        poh: &mut Poh,
    //        receiver: &Receiver<Option<Hash>>,
    //        sender: &Sender<PohEntry>,
    //    ) -> Result<(), ()> {
    //        loop {
    //            match receiver.recv() {
    //                Ok(hash) => Self::process_hash(hash, poh, sender)?,
    //                Err(RecvError) => return Err(()),
    //            }
    //        }
    //    }
    //
    //    fn try_process_hashes(
    //        poh: &mut Poh,
    //        receiver: &Receiver<Option<Hash>>,
    //        sender: &Sender<PohEntry>,
    //    ) -> Result<(), ()> {
    //        loop {
    //            match receiver.try_recv() {
    //                Ok(hash) => Self::process_hash(hash, poh, sender)?,
    //                Err(TryRecvError::Empty) => return Ok(()),
    //                Err(TryRecvError::Disconnected) => return Err(()),
    //            };
    //        }
    //    }
}

impl Service for PohService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.run_poh.store(false, Ordering::Relaxed);
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poh::verify;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_poh() {
        let poh_service = PohService::new(Hash::default(), false);

        let entry0 = poh_service.record(Hash::default());
        sleep(Duration::from_millis(1));
        let entry1 = poh_service.record(Hash::default());
        sleep(Duration::from_millis(1));
        let entry2 = poh_service.record(Hash::default());

        assert_eq!(entry0.num_hashes, 1);
        assert_eq!(entry1.num_hashes, 1);
        assert_eq!(entry2.num_hashes, 1);

        assert_eq!(poh_service.join().unwrap(), ());

        assert!(verify(Hash::default(), &[entry0, entry1, entry2]));
    }

    #[test]
    fn test_do_poh() {
        let poh_service = PohService::new(Hash::default(), true);

        sleep(Duration::from_millis(50));
        let entry = poh_service.tick();
        assert!(entry.num_hashes > 1);

        assert_eq!(poh_service.join().unwrap(), ());

        assert!(verify(Hash::default(), &vec![entry]));
    }
}
