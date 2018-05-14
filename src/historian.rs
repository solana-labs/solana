//! The `historian` module provides a microservice for generating a Proof of History.
//! It manages a thread containing a Proof of History Recorder.

use entry::Entry;
use hash::Hash;
use recorder::{ExitReason, Recorder, Signal};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::{spawn, JoinHandle};
use std::time::{Duration, Instant};

pub struct Historian {
    pub entry_receiver: Receiver<Entry>,
    pub thread_hdl: JoinHandle<ExitReason>,
}

impl Historian {
    pub fn new(
        event_receiver: Receiver<Signal>,
        start_hash: &Hash,
        tick_duration: Option<Duration>,
    ) -> Self {
        let (entry_sender, entry_receiver) = channel();
        let thread_hdl =
            Self::create_recorder(*start_hash, tick_duration, event_receiver, entry_sender);
        Historian {
            entry_receiver,
            thread_hdl,
        }
    }

    /// A background thread that will continue tagging received Event messages and
    /// sending back Entry messages until either the receiver or sender channel is closed.
    fn create_recorder(
        start_hash: Hash,
        tick_duration: Option<Duration>,
        receiver: Receiver<Signal>,
        sender: Sender<Entry>,
    ) -> JoinHandle<ExitReason> {
        spawn(move || {
            let mut recorder = Recorder::new(receiver, sender, start_hash);
            let duration_data = tick_duration.map(|dur| (Instant::now(), dur));
            loop {
                if let Err(err) = recorder.process_events(duration_data) {
                    return err;
                }
                if duration_data.is_some() {
                    recorder.hash();
                }
            }
        })
    }

    pub fn receive(self: &Self) -> Result<Entry, TryRecvError> {
        self.entry_receiver.try_recv()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::Block;
    use std::thread::sleep;

    #[test]
    fn test_historian() {
        let (input, event_receiver) = channel();
        let zero = Hash::default();
        let hist = Historian::new(event_receiver, &zero, None);

        input.send(Signal::Tick).unwrap();
        sleep(Duration::new(0, 1_000_000));
        input.send(Signal::Tick).unwrap();
        sleep(Duration::new(0, 1_000_000));
        input.send(Signal::Tick).unwrap();

        let entry0 = hist.entry_receiver.recv().unwrap();
        let entry1 = hist.entry_receiver.recv().unwrap();
        let entry2 = hist.entry_receiver.recv().unwrap();

        assert_eq!(entry0.num_hashes, 0);
        assert_eq!(entry1.num_hashes, 0);
        assert_eq!(entry2.num_hashes, 0);

        drop(input);
        assert_eq!(
            hist.thread_hdl.join().unwrap(),
            ExitReason::RecvDisconnected
        );

        assert!([entry0, entry1, entry2].verify(&zero));
    }

    #[test]
    fn test_historian_closed_sender() {
        let (input, event_receiver) = channel();
        let zero = Hash::default();
        let hist = Historian::new(event_receiver, &zero, None);
        drop(hist.entry_receiver);
        input.send(Signal::Tick).unwrap();
        assert_eq!(
            hist.thread_hdl.join().unwrap(),
            ExitReason::SendDisconnected
        );
    }

    #[test]
    #[ignore]
    fn test_ticking_historian() {
        let (input, event_receiver) = channel();
        let zero = Hash::default();
        let hist = Historian::new(event_receiver, &zero, Some(Duration::from_millis(20)));
        sleep(Duration::from_millis(900));
        input.send(Signal::Tick).unwrap();
        drop(input);
        let entries: Vec<Entry> = hist.entry_receiver.iter().collect();
        assert!(entries.len() > 1);

        // Ensure the ID is not the seed.
        assert_ne!(entries[0].id, zero);
    }
}
