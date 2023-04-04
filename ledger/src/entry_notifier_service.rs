use {
    crate::entry_notifier_interface::EntryNotifierLock,
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender},
    solana_entry::entry::EntrySummary,
    solana_sdk::clock::Slot,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub type EntryNotifierSender = Sender<(Slot, usize, EntrySummary)>;
pub type EntryNotifierReceiver = Receiver<(Slot, usize, EntrySummary)>;

pub struct EntryNotifierService {
    thread_hdl: JoinHandle<()>,
}

impl EntryNotifierService {
    pub fn new(
        entry_notification_receiver: EntryNotifierReceiver,
        entry_notifier: EntryNotifierLock,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();
        let thread_hdl = Builder::new()
            .name("solEntryNotif".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }

                if let Err(RecvTimeoutError::Disconnected) =
                    Self::notify_entry(&entry_notification_receiver, entry_notifier.clone())
                {
                    break;
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn notify_entry(
        entry_notification_receiver: &EntryNotifierReceiver,
        entry_notifier: EntryNotifierLock,
    ) -> Result<(), RecvTimeoutError> {
        let (slot, index, entry) =
            entry_notification_receiver.recv_timeout(Duration::from_secs(1))?;
        entry_notifier
            .write()
            .unwrap()
            .notify_entry(slot, index, &entry);
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
