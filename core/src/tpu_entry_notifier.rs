use {
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender},
    solana_entry::entry::EntrySummary,
    solana_ledger::entry_notifier_service::{EntryNotification, EntryNotifierSender},
    solana_poh::poh_recorder::WorkingBankEntry,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub(crate) struct TpuEntryNotifier {
    thread_hdl: JoinHandle<()>,
}

impl TpuEntryNotifier {
    pub(crate) fn new(
        entry_receiver: Receiver<WorkingBankEntry>,
        entry_notification_sender: EntryNotifierSender,
        broadcast_entry_sender: Sender<WorkingBankEntry>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solTpuEntry".to_string())
            .spawn(move || {
                let mut current_slot = 0;
                let mut current_index = 0;
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Err(RecvTimeoutError::Disconnected) = Self::send_entry_notification(
                        exit.clone(),
                        &entry_receiver,
                        &entry_notification_sender,
                        &broadcast_entry_sender,
                        &mut current_slot,
                        &mut current_index,
                    ) {
                        break;
                    }
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    pub(crate) fn send_entry_notification(
        exit: Arc<AtomicBool>,
        entry_receiver: &Receiver<WorkingBankEntry>,
        entry_notification_sender: &EntryNotifierSender,
        broadcast_entry_sender: &Sender<WorkingBankEntry>,
        current_slot: &mut u64,
        current_index: &mut usize,
    ) -> Result<(), RecvTimeoutError> {
        let (bank, (entry, tick_height)) = entry_receiver.recv_timeout(Duration::from_secs(1))?;
        let slot = bank.slot();
        let index = if slot != *current_slot {
            *current_index = 0;
            *current_slot = slot;
            0
        } else {
            *current_index += 1;
            *current_index
        };

        let entry_summary = EntrySummary {
            num_hashes: entry.num_hashes,
            hash: entry.hash,
            num_transactions: entry.transactions.len() as u64,
        };
        if let Err(err) = entry_notification_sender.send(EntryNotification {
            slot,
            index,
            entry: entry_summary,
        }) {
            warn!(
                "Failed to send slot {slot:?} entry {index:?} from Tpu to EntryNotifierService, error {err:?}",
            );
        }

        if let Err(err) = broadcast_entry_sender.send((bank, (entry, tick_height))) {
            warn!(
                "Failed to send slot {slot:?} entry {index:?} from Tpu to BroadcastStage, error {err:?}",
            );
            // If the BroadcastStage channel is closed, the validator has halted. Try to exit
            // gracefully.
            exit.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    pub(crate) fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
