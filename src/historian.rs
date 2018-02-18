//! The `historian` crate provides a microservice for generating a Proof-of-History.
//! It logs EventData items on behalf of its users. It continuously generates
//! new hashes, only stopping to check if it has been sent an EventData item. It
//! tags each EventData with an Event and sends it back. The Event includes the
//! EventData, the latest hash, and the number of hashes since the last event.
//! The resulting Event stream represents ordered events in time.

use std::thread::JoinHandle;
use std::sync::mpsc::{Receiver, Sender};
use event::{Event, EventData};

pub struct Historian {
    pub sender: Sender<EventData>,
    pub receiver: Receiver<Event>,
    pub thread_hdl: JoinHandle<(Event, EventThreadExitReason)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EventThreadExitReason {
    RecvDisconnected,
    SendDisconnected,
}

fn drain_queue(
    receiver: &Receiver<EventData>,
    sender: &Sender<Event>,
    num_hashes: u64,
    end_hash: u64,
) -> Result<u64, (Event, EventThreadExitReason)> {
    use std::sync::mpsc::TryRecvError;
    let mut num_hashes = num_hashes;
    loop {
        match receiver.try_recv() {
            Ok(data) => {
                let e = Event {
                    end_hash,
                    num_hashes,
                    data,
                };
                if let Err(_) = sender.send(e.clone()) {
                    return Err((e, EventThreadExitReason::SendDisconnected));
                }
                num_hashes = 0;
            }
            Err(TryRecvError::Empty) => {
                return Ok(num_hashes);
            }
            Err(TryRecvError::Disconnected) => {
                let e = Event {
                    end_hash,
                    num_hashes,
                    data: EventData::Tick,
                };
                return Err((e, EventThreadExitReason::RecvDisconnected));
            }
        }
    }
}

/// A background thread that will continue tagging received EventData messages and
/// sending back Event messages until either the receiver or sender channel is closed.
pub fn event_stream(
    start_hash: u64,
    receiver: Receiver<EventData>,
    sender: Sender<Event>,
) -> JoinHandle<(Event, EventThreadExitReason)> {
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
                Err(e) => return e,
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
        let (sender, event_data_receiver) = channel();
        let (event_sender, receiver) = channel();
        let thread_hdl = event_stream(start_hash, event_data_receiver, event_sender);
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

        let data = EventData::Tick;
        hist.sender.send(data.clone()).unwrap();
        let e0 = hist.receiver.recv().unwrap();
        assert_eq!(e0.data, data);

        let data = EventData::UserDataKey(0xdeadbeef);
        hist.sender.send(data.clone()).unwrap();
        let e1 = hist.receiver.recv().unwrap();
        assert_eq!(e1.data, data);

        drop(hist.sender);
        assert_eq!(
            hist.thread_hdl.join().unwrap().1,
            EventThreadExitReason::RecvDisconnected
        );

        verify_slice(&[e0, e1], 0);
    }

    #[test]
    fn test_historian_closed_sender() {
        let hist = Historian::new(0);
        drop(hist.receiver);
        hist.sender.send(EventData::Tick).unwrap();
        assert_eq!(
            hist.thread_hdl.join().unwrap().1,
            EventThreadExitReason::SendDisconnected
        );
    }
}
