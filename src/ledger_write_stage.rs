//! The `ledger_write_stage` module implements the ledger write stage. It
//! writes entries to the given writer, which is typically a file

use counter::Counter;
use entry::{EntryReceiver, EntrySender};
use ledger::LedgerWriter;
use log::Level;
use result::{Error, Result};
use service::Service;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::RecvTimeoutError;
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};
use timing::duration_as_ms;

pub struct LedgerWriteStage {
    write_thread: JoinHandle<()>,
}

impl LedgerWriteStage {
    pub fn write(
        ledger_writer: Option<&mut LedgerWriter>,
        entry_receiver: &EntryReceiver,
        forwarder: &Option<EntrySender>,
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
        if let Some(forwarder) = forwarder {
            for entries in ventries {
                forwarder.send(entries)?;
            }
        }
        inc_new_counter_info!(
            "ledger_writer_stage-time_ms",
            duration_as_ms(&now.elapsed()) as usize
        );
        Ok(())
    }

    pub fn new(
        ledger_path: Option<&str>,
        entry_receiver: EntryReceiver,
        forwarder: Option<EntrySender>,
    ) -> Self {
        let mut ledger_writer = ledger_path.map(|p| LedgerWriter::open(p, false).unwrap());

        let write_thread = Builder::new()
            .name("solana-ledger-writer".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::write(ledger_writer.as_mut(), &entry_receiver, &forwarder) {
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
            }).unwrap();

        LedgerWriteStage { write_thread }
    }
}

impl Service for LedgerWriteStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.write_thread.join()
    }
}
