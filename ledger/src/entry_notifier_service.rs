use {
    crate::entry_notifier_interface::EntryNotifierLock,
    crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender},
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

pub struct EntryNotification {
    pub slot: Slot,
    pub index: usize,
    pub entry: EntrySummary,
}

pub type EntryNotifierSender = Sender<EntryNotification>;
pub type EntryNotifierReceiver = Receiver<EntryNotification>;

pub struct EntryNotifierService {
    sender: EntryNotifierSender,
    thread_hdl: JoinHandle<()>,
}

impl EntryNotifierService {
    pub fn new(entry_notifier: EntryNotifierLock, exit: Arc<AtomicBool>) -> Self {
        let (entry_notification_sender, entry_notification_receiver) = unbounded();
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
        Self {
            sender: entry_notification_sender,
            thread_hdl,
        }
    }

    fn notify_entry(
        entry_notification_receiver: &EntryNotifierReceiver,
        entry_notifier: EntryNotifierLock,
    ) -> Result<(), RecvTimeoutError> {
        let EntryNotification { slot, index, entry } =
            entry_notification_receiver.recv_timeout(Duration::from_secs(1))?;
        entry_notifier
            .write()
            .unwrap()
            .notify_entry(slot, index, &entry);
        Ok(())
    }

    pub fn sender(&self) -> &EntryNotifierSender {
        &self.sender
    }

    pub fn sender_cloned(&self) -> EntryNotifierSender {
        self.sender.clone()
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
