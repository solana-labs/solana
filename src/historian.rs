//! The `historian` crate provides a microservice for generating a Proof-of-History.
//! It manages a thread containing a Proof-of-History Logger.

use std::thread::JoinHandle;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::Instant;
use log::{hash, Entry, Sha256Hash};
use logger::{verify_event_and_reserve_signature, ExitReason, Logger};
use event::{Event, Signature};
use serde::Serialize;
use std::fmt::Debug;

pub struct Historian<T> {
    pub sender: SyncSender<Event<T>>,
    pub receiver: Receiver<Entry<T>>,
    pub thread_hdl: JoinHandle<(Entry<T>, ExitReason)>,
    pub signatures: HashSet<Signature>,
}

impl<T: 'static + Serialize + Clone + Debug + Send> Historian<T> {
    pub fn new(start_hash: &Sha256Hash, ms_per_tick: Option<u64>) -> Self {
        use std::sync::mpsc::sync_channel;
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

    pub fn verify_event(self: &mut Self, event: &Event<T>) -> bool {
        return verify_event_and_reserve_signature(&mut self.signatures, event);
    }

    /// A background thread that will continue tagging received Event messages and
    /// sending back Entry messages until either the receiver or sender channel is closed.
    fn create_logger(
        start_hash: Sha256Hash,
        ms_per_tick: Option<u64>,
        receiver: Receiver<Event<T>>,
        sender: SyncSender<Entry<T>>,
    ) -> JoinHandle<(Entry<T>, ExitReason)> {
        use std::thread;
        thread::spawn(move || {
            let mut logger = Logger::new(receiver, sender, start_hash);
            let now = Instant::now();
            loop {
                if let Err(err) = logger.log_events(now, ms_per_tick) {
                    return err;
                }
                logger.end_hash = hash(&logger.end_hash);
                logger.num_hashes += 1;
            }
        })
    }
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
            hist.thread_hdl.join().unwrap().1,
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
            hist.thread_hdl.join().unwrap().1,
            ExitReason::SendDisconnected
        );
    }

    #[test]
    fn test_ticking_historian() {
        let zero = Sha256Hash::default();
        let hist = Historian::new(&zero, Some(20));
        sleep(Duration::from_millis(30));
        hist.sender.send(Event::Tick).unwrap();
        sleep(Duration::from_millis(15));
        drop(hist.sender);
        assert_eq!(
            hist.thread_hdl.join().unwrap().1,
            ExitReason::RecvDisconnected
        );

        let entries: Vec<Entry<Sha256Hash>> = hist.receiver.iter().collect();
        assert!(entries.len() > 1);
        assert!(verify_slice(&entries, &zero));
    }
}
