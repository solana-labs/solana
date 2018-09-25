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
use std::sync::mpsc::{channel, Receiver, RecvError, Sender, TryRecvError};
use std::thread::{self, Builder, JoinHandle};

pub struct PohService {
    thread_hdl: JoinHandle<()>,
}

impl PohService {
    /// A background thread that will continue tagging received Transaction messages and
    /// sending back Entry messages until either the receiver or sender channel is closed.
    /// if tick_duration is some, service will automatically produce entries every
    ///  `tick_duration`.
    pub fn new(
        start_hash: Hash,
        hash_receiver: Receiver<Option<Hash>>,
        do_poh: bool,
    ) -> (Self, Receiver<PohEntry>) {
        let (poh_sender, poh_receiver) = channel();

        let thread_hdl = Builder::new()
            .name("solana-record-service".to_string())
            .spawn(move || {
                let mut poh = Poh::new(start_hash);
                if do_poh {
                    loop {
                        if Self::try_process_hashes(&mut poh, &hash_receiver, &poh_sender).is_err()
                        {
                            return;
                        }
                        poh.hash();
                    }
                } else {
                    let _ = Self::process_hashes(&mut poh, &hash_receiver, &poh_sender);
                }
            }).unwrap();

        (PohService { thread_hdl }, poh_receiver)
    }

    fn process_hash(
        mixin: Option<Hash>,
        poh: &mut Poh,
        sender: &Sender<PohEntry>,
    ) -> Result<(), ()> {
        let resp = match mixin {
            Some(mixin) => poh.record(mixin),
            None => poh.tick(),
        };
        sender.send(resp).or(Err(()))?;
        Ok(())
    }

    fn process_hashes(
        poh: &mut Poh,
        receiver: &Receiver<Option<Hash>>,
        sender: &Sender<PohEntry>,
    ) -> Result<(), ()> {
        loop {
            match receiver.recv() {
                Ok(hash) => Self::process_hash(hash, poh, sender)?,
                Err(RecvError) => return Err(()),
            }
        }
    }

    fn try_process_hashes(
        poh: &mut Poh,
        receiver: &Receiver<Option<Hash>>,
        sender: &Sender<PohEntry>,
    ) -> Result<(), ()> {
        loop {
            match receiver.try_recv() {
                Ok(hash) => Self::process_hash(hash, poh, sender)?,
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(()),
            };
        }
    }
}

impl Service for PohService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poh::verify;
    use std::sync::mpsc::channel;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_poh() {
        use logger;
        logger::setup();
        let (hash_sender, hash_receiver) = channel();
        let (poh_service, poh_receiver) = PohService::new(Hash::default(), hash_receiver, false);

        hash_sender.send(Some(Hash::default())).unwrap();
        sleep(Duration::from_millis(1));
        hash_sender.send(None).unwrap();
        sleep(Duration::from_millis(1));
        hash_sender.send(Some(Hash::default())).unwrap();

        let entry0 = poh_receiver.recv().unwrap();
        let entry1 = poh_receiver.recv().unwrap();
        let entry2 = poh_receiver.recv().unwrap();

        assert_eq!(entry0.num_hashes, 1);
        assert_eq!(entry0.num_hashes, 1);
        assert_eq!(entry0.num_hashes, 1);

        drop(hash_sender);
        assert_eq!(poh_service.thread_hdl.join().unwrap(), ());

        assert!(verify(Hash::default(), &[entry0, entry1, entry2]));
    }

    #[test]
    fn test_poh_closed_sender() {
        use logger;
        logger::setup();
        let (hash_sender, hash_receiver) = channel();
        let (poh_service, poh_receiver) = PohService::new(Hash::default(), hash_receiver, false);
        drop(poh_receiver);
        hash_sender.send(Some(Hash::default())).unwrap();
        assert_eq!(poh_service.thread_hdl.join().unwrap(), ());
    }

    #[test]
    fn test_do_poh() {
        use logger;
        logger::setup();
        let (hash_sender, hash_receiver) = channel();
        let (_poh_service, poh_receiver) = PohService::new(Hash::default(), hash_receiver, true);

        sleep(Duration::from_millis(50));
        hash_sender.send(None).unwrap();
        drop(hash_sender);
        let pohs: Vec<_> = poh_receiver.iter().map(|x| x).collect();
        assert_eq!(pohs.len(), 1);
        assert!(pohs[0].num_hashes > 1);
        assert!(verify(Hash::default(), &pohs));
    }
}
