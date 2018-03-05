//! The `logger` crate provides an object for generating a Proof-of-History.
//! It logs Event items on behalf of its users. It continuously generates
//! new hashes, only stopping to check if it has been sent an Event item. It
//! tags each Event with an Entry and sends it back. The Entry includes the
//! Event, the latest hash, and the number of hashes since the last event.
//! The resulting stream of entries represents ordered events in time.

use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::time::{Duration, Instant};
use log::{create_entry_mut, Entry, Sha256Hash};
use event::Event;
use serde::Serialize;
use std::fmt::Debug;

#[derive(Debug, PartialEq, Eq)]
pub enum ExitReason {
    RecvDisconnected,
    SendDisconnected,
}

pub struct Logger<T> {
    pub sender: SyncSender<Entry<T>>,
    pub receiver: Receiver<Event<T>>,
    pub last_id: Sha256Hash,
    pub num_hashes: u64,
    pub num_ticks: u64,
}

impl<T: Serialize + Clone + Debug> Logger<T> {
    pub fn new(
        receiver: Receiver<Event<T>>,
        sender: SyncSender<Entry<T>>,
        start_hash: Sha256Hash,
    ) -> Self {
        Logger {
            receiver,
            sender,
            last_id: start_hash,
            num_hashes: 0,
            num_ticks: 0,
        }
    }

    pub fn log_event(&mut self, event: Event<T>) -> Result<Entry<T>, ExitReason> {
        let entry = create_entry_mut(&mut self.last_id, &mut self.num_hashes, event);
        Ok(entry)
    }

    pub fn process_events(
        &mut self,
        epoch: Instant,
        ms_per_tick: Option<u64>,
    ) -> Result<(), ExitReason> {
        loop {
            if let Some(ms) = ms_per_tick {
                if epoch.elapsed() > Duration::from_millis((self.num_ticks + 1) * ms) {
                    self.log_event(Event::Tick)?;
                    self.num_ticks += 1;
                }
            }

            match self.receiver.try_recv() {
                Ok(event) => {
                    let entry = self.log_event(event)?;
                    self.sender
                        .send(entry)
                        .or(Err(ExitReason::SendDisconnected))?;
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(ExitReason::RecvDisconnected),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::*;
    use event::*;
    use genesis::*;
    use std::sync::mpsc::sync_channel;

    #[test]
    fn test_bad_event_signature() {
        let keypair = generate_keypair();
        let sig = sign_claim_data(&hash(b"hello, world"), &keypair);
        let event0 = Event::new_claim(get_pubkey(&keypair), hash(b"goodbye cruel world"), sig);
        assert!(!verify_event(&event0));
    }

    fn run_genesis(gen: Genesis) -> Vec<Entry<u64>> {
        let (sender, event_receiver) = sync_channel(100);
        let (entry_sender, receiver) = sync_channel(100);
        let mut logger = Logger::new(event_receiver, entry_sender, hash(&gen.pkcs8));
        for tx in gen.create_events() {
            sender.send(tx).unwrap();
        }
        logger.process_events(Instant::now(), None).unwrap();
        drop(logger.sender);
        receiver.iter().collect::<Vec<_>>()
    }

    #[test]
    fn test_genesis_no_creators() {
        let entries = run_genesis(Genesis::new(100, vec![]));
        assert!(verify_slice_u64(&entries, &entries[0].id));
    }

    #[test]
    fn test_genesis() {
        let entries = run_genesis(Genesis::new(100, vec![Creator::new(42)]));
        assert!(verify_slice_u64(&entries, &entries[0].id));
    }
}
