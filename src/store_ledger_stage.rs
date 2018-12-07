//! The `store_ledger` stores the ledger from received entries for storage nodes

use counter::Counter;
use entry::EntryReceiver;
use ledger::LedgerWriter;
use log::Level;
use result::{Error, Result};
use service::Service;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::RecvTimeoutError;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

pub struct StoreLedgerStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl StoreLedgerStage {
    /// Process entries, already in order
    fn store_requests(
        window_receiver: &EntryReceiver,
        ledger_writer: Option<&mut LedgerWriter>,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let mut entries = window_receiver.recv_timeout(timer)?;
        while let Ok(mut more) = window_receiver.try_recv() {
            entries.append(&mut more);
        }

        inc_new_counter_info!(
            "store-transactions",
            entries.iter().map(|x| x.transactions.len()).sum()
        );

        if let Some(ledger_writer) = ledger_writer {
            ledger_writer.write_entries(&entries)?;
        }

        Ok(())
    }

    pub fn new(window_receiver: EntryReceiver, ledger_path: Option<&str>) -> Self {
        let mut ledger_writer = ledger_path.map(|p| LedgerWriter::open(p, true).unwrap());

        let t_store_requests = Builder::new()
            .name("solana-store-ledger-stage".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::store_requests(&window_receiver, ledger_writer.as_mut()) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap();

        let thread_hdls = vec![t_store_requests];

        StoreLedgerStage { thread_hdls }
    }
}

impl Service for StoreLedgerStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
