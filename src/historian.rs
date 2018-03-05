//! The `historian` crate provides a microservice for generating a Proof-of-History.
//! It manages a thread containing a Proof-of-History Logger.

use std::thread::{spawn, JoinHandle};
use std::collections::HashSet;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::time::Instant;
use log::{hash, Entry, Sha256Hash};
use logger::{ExitReason, Logger};
use event::{get_signature, Event, Signature};
use serde::Serialize;
use std::fmt::Debug;

pub struct Historian<T> {
    pub sender: SyncSender<Event<T>>,
    pub receiver: Receiver<Entry<T>>,
    pub thread_hdl: JoinHandle<ExitReason>,
    pub signatures: HashSet<Signature>,
}

impl<T: 'static + Serialize + Clone + Debug + Send> Historian<T> {
    pub fn new(start_hash: &Sha256Hash, ms_per_tick: Option<u64>) -> Self {
        let (sender, event_receiver) = sync_channel(1000);
        let (entry_sender, receiver) = sync_channel(1000);
        let thread_hdl =
            Historian::create_logger(*start_hash, ms_per_tick, event_receiver, entry_sender);
        let signatures = HashSet::new();
        Historian {
            sender,
            receiver,
            thread_hdl,
            signatures,
        }
    }

    /// A background thread that will continue tagging received Event messages and
    /// sending back Entry messages until either the receiver or sender channel is closed.
    fn create_logger(
        start_hash: Sha256Hash,
        ms_per_tick: Option<u64>,
        receiver: Receiver<Event<T>>,
        sender: SyncSender<Entry<T>>,
    ) -> JoinHandle<ExitReason> {
        spawn(move || {
            let mut logger = Logger::new(receiver, sender, start_hash);
            let now = Instant::now();
            loop {
                if let Err(err) = logger.process_events(now, ms_per_tick) {
                    return err;
                }
                logger.last_id = hash(&logger.last_id);
                logger.num_hashes += 1;
            }
        })
    }
}

pub fn reserve_signature<T>(sigs: &mut HashSet<Signature>, event: &Event<T>) -> bool {
    if let Some(sig) = get_signature(&event) {
        if sigs.contains(&sig) {
            return false;
        }
        sigs.insert(sig);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::*;
    use event::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_historian() {
        let zero = Sha256Hash::default();
        let hist = Historian::new(&zero, None);

        hist.sender.send(Event::Tick).unwrap();
        sleep(Duration::new(0, 1_000_000));
        hist.sender.send(Event::Tick).unwrap();
        sleep(Duration::new(0, 1_000_000));
        hist.sender.send(Event::Tick).unwrap();

        let entry0 = hist.receiver.recv().unwrap();
        let entry1 = hist.receiver.recv().unwrap();
        let entry2 = hist.receiver.recv().unwrap();

        drop(hist.sender);
        assert_eq!(
            hist.thread_hdl.join().unwrap(),
            ExitReason::RecvDisconnected
        );

        assert!(verify_slice(&[entry0, entry1, entry2], &zero));
    }

    #[test]
    fn test_historian_closed_sender() {
        let zero = Sha256Hash::default();
        let hist = Historian::<u8>::new(&zero, None);
        drop(hist.receiver);
        hist.sender.send(Event::Tick).unwrap();
        assert_eq!(
            hist.thread_hdl.join().unwrap(),
            ExitReason::SendDisconnected
        );
    }

    #[test]
    fn test_duplicate_event_signature() {
        let keypair = generate_keypair();
        let to = get_pubkey(&keypair);
        let data = b"hello, world";
        let zero = Sha256Hash::default();
        let sig = sign_claim_data(&data, &keypair, &zero);
        let event0 = Event::new_claim(to, &data, zero, sig);
        let mut sigs = HashSet::new();
        assert!(reserve_signature(&mut sigs, &event0));
        assert!(!reserve_signature(&mut sigs, &event0));
    }

    #[test]
    fn test_ticking_historian() {
        let zero = Sha256Hash::default();
        let hist = Historian::new(&zero, Some(20));
        sleep(Duration::from_millis(30));
        hist.sender.send(Event::Tick).unwrap();
        drop(hist.sender);
        let entries: Vec<Entry<Sha256Hash>> = hist.receiver.iter().collect();

        // Ensure one entry is sent back for each tick sent in.
        assert_eq!(entries.len(), 1);

        // Ensure the ID is not the seed, which indicates another Tick
        // was logged before the one we sent.
        assert_ne!(entries[0].id, zero);
    }
}
