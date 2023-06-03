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
        receiver: Receiver<WorkingBankEntry>,
        entry_notification_sender: EntryNotifierSender,
        broadcast_entry_sender: Sender<WorkingBankEntry>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solTpuEntry".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }

                if let Err(RecvTimeoutError::Disconnected) = Self::send_entry_notification(
                    &receiver,
                    &entry_notification_sender,
                    &broadcast_entry_sender,
                ) {
                    break;
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    pub(crate) fn send_entry_notification(
        receiver: &Receiver<WorkingBankEntry>,
        entry_notification_sender: &EntryNotifierSender,
        broadcast_entry_sender: &Sender<WorkingBankEntry>,
    ) -> Result<(), RecvTimeoutError> {
        let working_bank_entry = receiver.recv_timeout(Duration::from_secs(1))?;
        let slot = working_bank_entry.bank.slot();
        let index = working_bank_entry.entry_index;

        let entry_summary = EntrySummary {
            num_hashes: working_bank_entry.entry.num_hashes,
            hash: working_bank_entry.entry.hash,
            num_transactions: working_bank_entry.entry.transactions.len() as u64,
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

        // TODO: in PohRecorder, we panic if the send to BroadcastStage fails. Should we do the same here?
        if let Err(err) = broadcast_entry_sender.send(working_bank_entry) {
            warn!(
                "Failed to send slot {slot:?} entry {index:?} from Tpu to BroadcastStage, error {err:?}",
            );
        }
        Ok(())
    }

    pub(crate) fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
