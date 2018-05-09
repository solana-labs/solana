//! The `recorder` module provides an object for generating a Proof of History.
//! It records Event items on behalf of its users. It continuously generates
//! new hashes, only stopping to check if it has been sent an Event item. It
//! tags each Event with an Entry, and sends it back. The Entry includes the
//! Event, the latest hash, and the number of hashes since the last event.
//! The resulting stream of entries represents ordered events in time.

use entry::{create_entry_mut, Entry};
use event::Event;
use hash::{hash, Hash};
use std::mem;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::time::{Duration, Instant};

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
pub enum Signal {
    Tick,
    Event(Event),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExitReason {
    RecvDisconnected,
    SendDisconnected,
}

pub struct Recorder {
    sender: SyncSender<Entry>,
    receiver: Receiver<Signal>,
    last_hash: Hash,
    events: Vec<Event>,
    num_hashes: u64,
    num_ticks: u64,
}

impl Recorder {
    pub fn new(receiver: Receiver<Signal>, sender: SyncSender<Entry>, last_hash: Hash) -> Self {
        Recorder {
            receiver,
            sender,
            last_hash,
            events: vec![],
            num_hashes: 0,
            num_ticks: 0,
        }
    }

    pub fn hash(&mut self) {
        self.last_hash = hash(&self.last_hash);
        self.num_hashes += 1;
    }

    pub fn record_entry(&mut self) -> Result<(), ExitReason> {
        let events = mem::replace(&mut self.events, vec![]);
        let entry = create_entry_mut(&mut self.last_hash, &mut self.num_hashes, events);
        self.sender
            .send(entry)
            .or(Err(ExitReason::SendDisconnected))?;
        Ok(())
    }

    pub fn process_events(
        &mut self,
        epoch: Instant,
        ms_per_tick: Option<u64>,
    ) -> Result<(), ExitReason> {
        loop {
            if let Some(ms) = ms_per_tick {
                if epoch.elapsed() > Duration::from_millis((self.num_ticks + 1) * ms) {
                    self.record_entry()?;
                    self.num_ticks += 1;
                }
            }

            match self.receiver.try_recv() {
                Ok(signal) => match signal {
                    Signal::Tick => {
                        self.record_entry()?;
                    }
                    Signal::Event(event) => {
                        self.events.push(event);
                    }
                },
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(ExitReason::RecvDisconnected),
            };
        }
    }
}

//#[cfg(test)]
//mod tests {
//    use super::*;
//    use bincode::serialize;
//    use signature::{KeyPair, KeyPairUtil};
//    use std::sync::mpsc::sync_channel;
//    use transaction::Transaction;
//
//    #[test]
//    fn test_events() {
//        let (signal_sender, signal_receiver) = sync_channel(500);
//        let (entry_sender, entry_receiver) = sync_channel(10);
//        let zero = Hash::default();
//        let mut recorder = Recorder::new(signal_receiver, entry_sender, zero);
//        let alice_keypair = KeyPair::new();
//        let bob_pubkey = KeyPair::new().pubkey();
//        let event0 = Event::Transaction(Transaction::new(&alice_keypair, bob_pubkey, 1, zero));
//        let event1 = Event::Transaction(Transaction::new(&alice_keypair, bob_pubkey, 2, zero));
//        signal_sender
//            .send(Signal::Events(vec![event0, event1]))
//            .unwrap();
//        recorder.process_events(Instant::now(), None).unwrap();
//
//        drop(recorder.sender);
//        let entries: Vec<_> = entry_receiver.iter().collect();
//        assert_eq!(entries.len(), 1);
//    }
//}
