//! The `historian` crate provides a microservice for generating a Proof-of-History.
//! It logs Event items on behalf of its users. It continuously generates
//! new hashes, only stopping to check if it has been sent an Event item. It
//! tags each Event with an Entry and sends it back. The Entry includes the
//! Event, the latest hash, and the number of hashes since the last event.
//! The resulting stream of entries represents ordered events in time.

use std::thread::JoinHandle;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::{Duration, Instant};
use log::{hash, hash_event, Entry, Sha256Hash};
use event::{get_signature, verify_event, Event, Signature};
use serde::Serialize;
use std::fmt::Debug;

pub struct Historian<T> {
    pub sender: SyncSender<Event<T>>,
    pub receiver: Receiver<Entry<T>>,
    pub thread_hdl: JoinHandle<(Entry<T>, ExitReason)>,
    pub signatures: HashSet<Signature>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExitReason {
    RecvDisconnected,
    SendDisconnected,
}

pub struct Logger<T> {
    pub sender: SyncSender<Entry<T>>,
    pub receiver: Receiver<Event<T>>,
    pub end_hash: Sha256Hash,
    pub num_hashes: u64,
    pub num_ticks: u64,
}

fn verify_event_and_reserve_signature<T: Serialize>(
    signatures: &mut HashSet<Signature>,
    event: &Event<T>,
) -> bool {
    if !verify_event(&event) {
        return false;
    }
    if let Some(sig) = get_signature(&event) {
        if signatures.contains(&sig) {
            return false;
        }
        signatures.insert(sig);
    }
    true
}

impl<T: Serialize + Clone + Debug> Logger<T> {
    fn new(
        receiver: Receiver<Event<T>>,
        sender: SyncSender<Entry<T>>,
        start_hash: Sha256Hash,
    ) -> Self {
        Logger {
            receiver,
            sender,
            end_hash: start_hash,
            num_hashes: 0,
            num_ticks: 0,
        }
    }

    fn log_event(&mut self, event: Event<T>) -> Result<(), (Entry<T>, ExitReason)> {
        self.end_hash = hash_event(&self.end_hash, &event);
        let entry = Entry {
            end_hash: self.end_hash,
            num_hashes: self.num_hashes,
            event,
        };
        if let Err(_) = self.sender.send(entry.clone()) {
            return Err((entry, ExitReason::SendDisconnected));
        }
        self.num_hashes = 0;
        Ok(())
    }

    fn log_events(
        &mut self,
        epoch: Instant,
        ms_per_tick: Option<u64>,
    ) -> Result<(), (Entry<T>, ExitReason)> {
        use std::sync::mpsc::TryRecvError;
        loop {
            if let Some(ms) = ms_per_tick {
                if epoch.elapsed() > Duration::from_millis((self.num_ticks + 1) * ms) {
                    self.log_event(Event::Tick)?;
                    self.num_ticks += 1;
                }
            }
            match self.receiver.try_recv() {
                Ok(event) => {
                    self.log_event(event)?;
                }
                Err(TryRecvError::Empty) => {
                    return Ok(());
                }
                Err(TryRecvError::Disconnected) => {
                    let entry = Entry {
                        end_hash: self.end_hash,
                        num_hashes: self.num_hashes,
                        event: Event::Tick,
                    };
                    return Err((entry, ExitReason::RecvDisconnected));
                }
            }
        }
    }
}

/// A background thread that will continue tagging received Event messages and
/// sending back Entry messages until either the receiver or sender channel is closed.
pub fn create_logger<T: 'static + Serialize + Clone + Debug + Send>(
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

impl<T: 'static + Serialize + Clone + Debug + Send> Historian<T> {
    pub fn new(start_hash: &Sha256Hash, ms_per_tick: Option<u64>) -> Self {
        use std::sync::mpsc::sync_channel;
        let (sender, event_receiver) = sync_channel(1000);
        let (entry_sender, receiver) = sync_channel(1000);
        let thread_hdl = create_logger(*start_hash, ms_per_tick, event_receiver, entry_sender);
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

    #[test]
    fn test_bad_event_signature() {
        let keypair = generate_keypair();
        let sig = sign_claim_data(&hash(b"hello, world"), &keypair);
        let event0 = Event::new_claim(get_pubkey(&keypair), hash(b"goodbye cruel world"), sig);
        let mut sigs = HashSet::new();
        assert!(!verify_event_and_reserve_signature(&mut sigs, &event0));
        assert!(!sigs.contains(&sig));
    }

    #[test]
    fn test_duplicate_event_signature() {
        let keypair = generate_keypair();
        let to = get_pubkey(&keypair);
        let data = &hash(b"hello, world");
        let sig = sign_claim_data(data, &keypair);
        let event0 = Event::new_claim(to, data, sig);
        let mut sigs = HashSet::new();
        assert!(verify_event_and_reserve_signature(&mut sigs, &event0));
        assert!(!verify_event_and_reserve_signature(&mut sigs, &event0));
    }
}
