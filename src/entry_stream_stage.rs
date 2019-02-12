//! The `entry_stream_stage` implements optional streaming of entries using the
//! `entry_stream` module, providing client services such as a block explorer with
//! real-time access to entries.

use crate::entry::{EntryReceiver, EntrySender};
use crate::entry_stream::EntryStreamHandler;
#[cfg(test)]
use crate::entry_stream::MockEntryStream as EntryStream;
#[cfg(not(test))]
use crate::entry_stream::SocketEntryStream as EntryStream;
use crate::leader_scheduler::LeaderScheduler;
use crate::result::{Error, Result};
use crate::service::Service;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

pub struct EntryStreamStage {
    t_entry_stream: JoinHandle<()>,
}

impl EntryStreamStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        ledger_entry_receiver: EntryReceiver,
        entry_stream_socket: String,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        exit: Arc<AtomicBool>,
    ) -> (Self, EntryReceiver) {
        let (entry_stream_sender, entry_stream_receiver) = channel();
        let mut entry_stream = EntryStream::new(entry_stream_socket, leader_scheduler);
        let t_entry_stream = Builder::new()
            .name("solana-entry-stream".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = Self::process_entries(
                    &ledger_entry_receiver,
                    &entry_stream_sender,
                    &mut entry_stream,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => info!("Error from process_entries: {:?}", e),
                    }
                }
            })
            .unwrap();
        (Self { t_entry_stream }, entry_stream_receiver)
    }
    fn process_entries(
        ledger_entry_receiver: &EntryReceiver,
        entry_stream_sender: &EntrySender,
        entry_stream: &mut EntryStream,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let entries = ledger_entry_receiver.recv_timeout(timeout)?;
        entry_stream
            .emit_entry_events(&entries)
            .unwrap_or_else(|e| {
                error!("Entry Stream error: {:?}, {:?}", e, entry_stream.output);
            });
        entry_stream_sender.send(entries)?;
        Ok(())
    }
}

impl Service for EntryStreamStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_entry_stream.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bank::Bank;
    use crate::entry::Entry;
    use crate::genesis_block::GenesisBlock;
    use crate::leader_scheduler::LeaderSchedulerConfig;
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_sdk::hash::Hash;

    #[test]
    fn test_entry_stream_stage_process_entries() {
        // Set up bank and leader_scheduler
        let leader_scheduler_config = LeaderSchedulerConfig::new(5, 2, 10);
        let (genesis_block, _mint_keypair) = GenesisBlock::new(1_000_000);
        let bank = Bank::new_with_leader_scheduler_config(&genesis_block, &leader_scheduler_config);
        // Set up entry stream
        let mut entry_stream =
            EntryStream::new("test_stream".to_string(), bank.leader_scheduler.clone());

        // Set up dummy channels to host an EntryStreamStage
        let (ledger_entry_sender, ledger_entry_receiver) = channel();
        let (entry_stream_sender, entry_stream_receiver) = channel();

        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        let mut expected_entries = Vec::new();
        for x in 0..5 {
            let entry = Entry::new(&mut last_id, x, 1, vec![]); //just ticks
            last_id = entry.id;
            expected_entries.push(entry.clone());
            entries.push(entry);
        }
        ledger_entry_sender.send(entries).unwrap();
        EntryStreamStage::process_entries(
            &ledger_entry_receiver,
            &entry_stream_sender,
            &mut entry_stream,
        )
        .unwrap();
        assert_eq!(entry_stream.entries().len(), 5);

        for (i, item) in entry_stream.entries().iter().enumerate() {
            let json: Value = serde_json::from_str(&item).unwrap();
            let dt_str = json["dt"].as_str().unwrap();

            // Ensure `ts` field parses as valid DateTime
            let _dt: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(dt_str).unwrap();

            let entry_obj = json["entry"].clone();
            let entry: Entry = serde_json::from_value(entry_obj).unwrap();
            assert_eq!(entry, expected_entries[i]);
        }
        // Ensure entries pass through stage unadulterated
        let recv_entries = entry_stream_receiver.recv().unwrap();
        assert_eq!(expected_entries, recv_entries);
    }
}
