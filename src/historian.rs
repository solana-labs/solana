//! The `historian` crate provides a microservice for generating a Proof-of-History.
//! It logs Event items on behalf of its users. It continuously generates
//! new hashes, only stopping to check if it has been sent an Event item. It
//! tags each Event with an Entry and sends it back. The Entry includes the
//! Event, the latest hash, and the number of hashes since the last event.
//! The resulting stream of entries represents ordered events in time.

use std::thread::JoinHandle;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::{Duration, SystemTime};
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
fn log_event<T: Serialize + Clone + Debug>(
    sender: &SyncSender<Entry<T>>,
    num_hashes: &mut u64,
    end_hash: &mut Sha256Hash,
    event: Event<T>,
) -> Result<(), (Entry<T>, ExitReason)> {
    *end_hash = hash_event(end_hash, &event);
    let entry = Entry {
        end_hash: *end_hash,
        num_hashes: *num_hashes,
        event,
    };
    if let Err(_) = sender.send(entry.clone()) {
        return Err((entry, ExitReason::SendDisconnected));
    }
    *num_hashes = 0;
    Ok(())
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

fn log_events<T: Serialize + Clone + Debug>(
    receiver: &Receiver<Event<T>>,
    sender: &SyncSender<Entry<T>>,
    num_hashes: &mut u64,
    end_hash: &mut Sha256Hash,
    epoch: SystemTime,
    num_ticks: &mut u64,
    ms_per_tick: Option<u64>,
) -> Result<(), (Entry<T>, ExitReason)> {
    use std::sync::mpsc::TryRecvError;
    loop {
        if let Some(ms) = ms_per_tick {
            let now = SystemTime::now();
            if now > epoch + Duration::from_millis((*num_ticks + 1) * ms) {
                log_event(sender, num_hashes, end_hash, Event::Tick)?;
                *num_ticks += 1;
            }
        }
        match receiver.try_recv() {
            Ok(event) => {
                log_event(sender, num_hashes, end_hash, event)?;
            }
            Err(TryRecvError::Empty) => {
                return Ok(());
            }
            Err(TryRecvError::Disconnected) => {
                let entry = Entry {
                    end_hash: *end_hash,
                    num_hashes: *num_hashes,
                    event: Event::Tick,
                };
                return Err((entry, ExitReason::RecvDisconnected));
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
        let mut end_hash = start_hash;
        let mut num_hashes = 0;
        let mut num_ticks = 0;
        let epoch = SystemTime::now();
        loop {
            if let Err(err) = log_events(
                &receiver,
                &sender,
                &mut num_hashes,
                &mut end_hash,
                epoch,
                &mut num_ticks,
                ms_per_tick,
            ) {
                return err;
            }
            end_hash = hash(&end_hash);
            num_hashes += 1;
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
