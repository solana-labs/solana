//! The `ledger_write_stage` module implements the ledger write stage. It
//! writes entries to the given writer, which is typically a file

use crate::counter::Counter;
use crate::entry::{EntryReceiver, EntrySender};
use crate::ledger::LedgerWriter;
use crate::result::{Error, Result};
use crate::service::Service;
use log::Level;
use solana_sdk::timing::duration_as_ms;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};

pub struct LedgerWriteStage {
    write_thread: JoinHandle<()>,
}

impl LedgerWriteStage {
    pub fn write(
        ledger_writer: Option<&mut LedgerWriter>,
        entry_receiver: &EntryReceiver,
        entry_sender: &EntrySender,
    ) -> Result<()> {
        let mut ventries = Vec::new();
        let mut received_entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        let mut num_new_entries = 0;
        let now = Instant::now();

        loop {
            num_new_entries += received_entries.len();
            ventries.push(received_entries);

            if let Ok(n) = entry_receiver.try_recv() {
                received_entries = n;
            } else {
                break;
            }
        }

        if let Some(ledger_writer) = ledger_writer {
            ledger_writer.write_entries(ventries.iter().flatten())?;
        }

        inc_new_counter_info!("ledger_writer_stage-entries_received", num_new_entries);
        for entries in ventries {
            entry_sender.send(entries)?;
        }
        inc_new_counter_info!(
            "ledger_writer_stage-time_ms",
            duration_as_ms(&now.elapsed()) as usize
        );
        Ok(())
    }

    #[allow(clippy::new_ret_no_self)]
    pub fn new(ledger_path: Option<&str>, entry_receiver: EntryReceiver) -> (Self, EntryReceiver) {
        let mut ledger_writer = ledger_path.map(|p| LedgerWriter::open(p, false).unwrap());

        let (entry_sender, entry_forwarder) = channel();
        let write_thread = Builder::new()
            .name("solana-ledger-writer".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::write(ledger_writer.as_mut(), &entry_receiver, &entry_sender)
                {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                            break;
                        }
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => {
                            inc_new_counter_info!(
                                "ledger_writer_stage-write_and_send_entries-error",
                                1
                            );
                            error!("{:?}", e);
                        }
                    }
                };
            })
            .unwrap();

        (Self { write_thread }, entry_forwarder)
    }
}

impl Service for LedgerWriteStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.write_thread.join()
    }
}
