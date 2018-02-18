//! The `historian` crate provides a microservice for generating a Proof-of-History.
//! It logs Event items on behalf of its users. It continuously generates
//! new hashes, only stopping to check if it has been sent an Event item. It
//! tags each Event with an Entry and sends it back. The Entry includes the
//! Event, the latest hash, and the number of hashes since the last event.
//! The resulting stream of entries represents ordered events in time.

use std::thread::JoinHandle;
use std::sync::mpsc::{Receiver, Sender};
use event::{Entry, Event};

pub struct Historian {
    pub sender: Sender<Event>,
    pub receiver: Receiver<Entry>,
    pub thread_hdl: JoinHandle<(Entry, ExitReason)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExitReason {
    RecvDisconnected,
    SendDisconnected,
}

fn drain_queue(
    receiver: &Receiver<Event>,
    sender: &Sender<Entry>,
    num_hashes: u64,
    end_hash: u64,
) -> Result<u64, (Entry, ExitReason)> {
    use std::sync::mpsc::TryRecvError;
    let mut num_hashes = num_hashes;
    loop {
        match receiver.try_recv() {
            Ok(event) => {
                let entry = Entry {
                    end_hash,
                    num_hashes,
                    event,
                };
                if let Err(_) = sender.send(entry.clone()) {
                    return Err((entry, ExitReason::SendDisconnected));
                }
                num_hashes = 0;
            }
            Err(TryRecvError::Empty) => {
                return Ok(num_hashes);
            }
            Err(TryRecvError::Disconnected) => {
                let entry = Entry {
                    end_hash,
                    num_hashes,
                    event: Event::Tick,
                };
                return Err((entry, ExitReason::RecvDisconnected));
            }
        }
    }
}

/// A background thread that will continue tagging received Event messages and
/// sending back Entry messages until either the receiver or sender channel is closed.
pub fn event_stream(
    start_hash: u64,
    receiver: Receiver<Event>,
    sender: Sender<Entry>,
) -> JoinHandle<(Entry, ExitReason)> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::thread;
    thread::spawn(move || {
        let mut end_hash = start_hash;
        let mut hasher = DefaultHasher::new();
        let mut num_hashes = 0;
        loop {
            match drain_queue(&receiver, &sender, num_hashes, end_hash) {
                Ok(n) => num_hashes = n,
                Err(err) => return err,
            }
            end_hash.hash(&mut hasher);
            end_hash = hasher.finish();
            num_hashes += 1;
        }
    })
}

impl Historian {
    pub fn new(start_hash: u64) -> Self {
        use std::sync::mpsc::channel;
        let (sender, event_receiver) = channel();
        let (entry_sender, receiver) = channel();
        let thread_hdl = event_stream(start_hash, event_receiver, entry_sender);
        Historian {
            sender,
            receiver,
            thread_hdl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use event::*;

    #[test]
    fn test_historian() {
        let hist = Historian::new(0);

        let event = Event::Tick;
        hist.sender.send(event.clone()).unwrap();
        let entry0 = hist.receiver.recv().unwrap();
        assert_eq!(entry0.event, event);

        let event = Event::UserDataKey(0xdeadbeef);
        hist.sender.send(event.clone()).unwrap();
        let entry1 = hist.receiver.recv().unwrap();
        assert_eq!(entry1.event, event);

        drop(hist.sender);
        assert_eq!(
            hist.thread_hdl.join().unwrap().1,
            ExitReason::RecvDisconnected
        );

        verify_slice(&[entry0, entry1], 0);
    }

    #[test]
    fn test_historian_closed_sender() {
        let hist = Historian::new(0);
        drop(hist.receiver);
        hist.sender.send(Event::Tick).unwrap();
        assert_eq!(
            hist.thread_hdl.join().unwrap().1,
            ExitReason::SendDisconnected
        );
    }
}
