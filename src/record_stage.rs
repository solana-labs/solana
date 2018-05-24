//! The `record_stage` module provides an object for generating a Proof of History.
//! It records Event items on behalf of its users. It continuously generates
//! new hashes, only stopping to check if it has been sent an Event item. It
//! tags each Event with an Entry, and sends it back. The Entry includes the
//! Event, the latest hash, and the number of hashes since the last event.
//! The resulting stream of entries represents ordered events in time.

use entry::Entry;
use event::Event;
use hash::Hash;
use recorder::Recorder;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::{spawn, JoinHandle};
use std::time::{Duration, Instant};

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
pub enum Signal {
    Tick,
    Events(Vec<Event>),
}

pub struct RecordStage {
    pub entry_receiver: Receiver<Entry>,
    pub thread_hdl: JoinHandle<()>,
}

impl RecordStage {
    /// A background thread that will continue tagging received Event messages and
    /// sending back Entry messages until either the receiver or sender channel is closed.
    pub fn new(
        event_receiver: Receiver<Signal>,
        start_hash: &Hash,
        tick_duration: Option<Duration>,
    ) -> Self {
        let (entry_sender, entry_receiver) = channel();
        let start_hash = start_hash.clone();

        let thread_hdl = spawn(move || {
            let mut recorder = Recorder::new(start_hash);
            let duration_data = tick_duration.map(|dur| (Instant::now(), dur));
            loop {
                if let Err(_) = Self::process_events(
                    &mut recorder,
                    duration_data,
                    &event_receiver,
                    &entry_sender,
                ) {
                    return;
                }
                if duration_data.is_some() {
                    recorder.hash();
                }
            }
        });

        RecordStage {
            entry_receiver,
            thread_hdl,
        }
    }

    pub fn process_events(
        recorder: &mut Recorder,
        duration_data: Option<(Instant, Duration)>,
        receiver: &Receiver<Signal>,
        sender: &Sender<Entry>,
    ) -> Result<(), ()> {
        loop {
            if let Some((start_time, tick_duration)) = duration_data {
                if let Some(entry) = recorder.tick(start_time, tick_duration) {
                    sender.send(entry).or(Err(()))?;
                }
            }
            match receiver.try_recv() {
                Ok(signal) => match signal {
                    Signal::Tick => {
                        let entry = recorder.record(vec![]);
                        sender.send(entry).or(Err(()))?;
                    }
                    Signal::Events(events) => {
                        let entry = recorder.record(events);
                        sender.send(entry).or(Err(()))?;
                    }
                },
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(()),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::Block;
    use signature::{KeyPair, KeyPairUtil};
    use std::sync::mpsc::channel;
    use std::thread::sleep;

    #[test]
    fn test_historian() {
        let (input, event_receiver) = channel();
        let zero = Hash::default();
        let record_stage = RecordStage::new(event_receiver, &zero, None);

        input.send(Signal::Tick).unwrap();
        sleep(Duration::new(0, 1_000_000));
        input.send(Signal::Tick).unwrap();
        sleep(Duration::new(0, 1_000_000));
        input.send(Signal::Tick).unwrap();

        let entry0 = record_stage.entry_receiver.recv().unwrap();
        let entry1 = record_stage.entry_receiver.recv().unwrap();
        let entry2 = record_stage.entry_receiver.recv().unwrap();

        assert_eq!(entry0.num_hashes, 0);
        assert_eq!(entry1.num_hashes, 0);
        assert_eq!(entry2.num_hashes, 0);

        drop(input);
        assert_eq!(record_stage.thread_hdl.join().unwrap(), ());

        assert!([entry0, entry1, entry2].verify(&zero));
    }

    #[test]
    fn test_historian_closed_sender() {
        let (input, event_receiver) = channel();
        let zero = Hash::default();
        let record_stage = RecordStage::new(event_receiver, &zero, None);
        drop(record_stage.entry_receiver);
        input.send(Signal::Tick).unwrap();
        assert_eq!(record_stage.thread_hdl.join().unwrap(), ());
    }

    #[test]
    fn test_events() {
        let (input, signal_receiver) = channel();
        let zero = Hash::default();
        let record_stage = RecordStage::new(signal_receiver, &zero, None);
        let alice_keypair = KeyPair::new();
        let bob_pubkey = KeyPair::new().pubkey();
        let event0 = Event::new_transaction(&alice_keypair, bob_pubkey, 1, zero);
        let event1 = Event::new_transaction(&alice_keypair, bob_pubkey, 2, zero);
        input.send(Signal::Events(vec![event0, event1])).unwrap();
        drop(input);
        let entries: Vec<_> = record_stage.entry_receiver.iter().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    #[ignore]
    fn test_ticking_historian() {
        let (input, event_receiver) = channel();
        let zero = Hash::default();
        let record_stage = RecordStage::new(event_receiver, &zero, Some(Duration::from_millis(20)));
        sleep(Duration::from_millis(900));
        input.send(Signal::Tick).unwrap();
        drop(input);
        let entries: Vec<Entry> = record_stage.entry_receiver.iter().collect();
        assert!(entries.len() > 1);

        // Ensure the ID is not the seed.
        assert_ne!(entries[0].id, zero);
    }
}
