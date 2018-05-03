//! The `historian` module provides a microservice for generating a Proof of History.
//! It manages a thread containing a Proof of History Recorder.

use entry::Entry;
use hash::Hash;
use recorder::{ExitReason, Recorder, Signal};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::{spawn, JoinHandle};
use std::time::Instant;

pub struct Historian {
    pub output: Receiver<Entry>,
    pub thread_hdl: JoinHandle<ExitReason>,
}

impl Historian {
    pub fn new(
        event_receiver: Receiver<Signal>,
        start_hash: &Hash,
        ms_per_tick: Option<u64>,
    ) -> Self {
        let (entry_sender, output) = sync_channel(10_000);
        let thread_hdl =
            Historian::create_recorder(*start_hash, ms_per_tick, event_receiver, entry_sender);
        Historian { output, thread_hdl }
    }

    /// A background thread that will continue tagging received Event messages and
    /// sending back Entry messages until either the receiver or sender channel is closed.
    fn create_recorder(
        start_hash: Hash,
        ms_per_tick: Option<u64>,
        receiver: Receiver<Signal>,
        sender: SyncSender<Entry>,
    ) -> JoinHandle<ExitReason> {
        spawn(move || {
            let mut recorder = Recorder::new(receiver, sender, start_hash);
            let now = Instant::now();
            loop {
                if let Err(err) = recorder.process_events(now, ms_per_tick) {
                    return err;
                }
                if ms_per_tick.is_some() {
                    recorder.hash();
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::Block;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_historian() {
        let (input, event_receiver) = sync_channel(10);
        let zero = Hash::default();
        let hist = Historian::new(event_receiver, &zero, None);

        input.send(Signal::Tick).unwrap();
        sleep(Duration::new(0, 1_000_000));
        input.send(Signal::Tick).unwrap();
        sleep(Duration::new(0, 1_000_000));
        input.send(Signal::Tick).unwrap();

        let entry0 = hist.output.recv().unwrap();
        let entry1 = hist.output.recv().unwrap();
        let entry2 = hist.output.recv().unwrap();

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
        let (input, event_receiver) = sync_channel(10);
        let zero = Hash::default();
        let hist = Historian::new(event_receiver, &zero, None);
        drop(hist.output);
        input.send(Signal::Tick).unwrap();
        assert_eq!(
            hist.thread_hdl.join().unwrap(),
            ExitReason::SendDisconnected
        );
    }

    #[test]
    fn test_ticking_historian() {
        let (input, event_receiver) = sync_channel(10);
        let zero = Hash::default();
        let hist = Historian::new(event_receiver, &zero, Some(20));
        sleep(Duration::from_millis(300));
        input.send(Signal::Tick).unwrap();
        drop(input);
        let entries: Vec<Entry> = hist.output.iter().collect();
        assert!(entries.len() > 1);

        // Ensure the ID is not the seed.
        assert_ne!(entries[0].id, zero);
    }
}
