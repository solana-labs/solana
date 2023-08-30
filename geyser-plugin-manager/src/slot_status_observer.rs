use {
    crate::slot_status_notifier::SlotStatusNotifier,
    crossbeam_channel::Receiver,
    solana_rpc::optimistically_confirmed_bank_tracker::SlotNotification,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
    },
};

#[derive(Debug)]
pub(crate) struct SlotStatusObserver {
    bank_notification_receiver_service: Option<JoinHandle<()>>,
    exit_updated_slot_server: Arc<AtomicBool>,
}

impl SlotStatusObserver {
    pub fn new(
        bank_notification_receiver: Receiver<SlotNotification>,
        slot_status_notifier: SlotStatusNotifier,
    ) -> Self {
        let exit_updated_slot_server = Arc::new(AtomicBool::new(false));

        Self {
            bank_notification_receiver_service: Some(Self::run_bank_notification_receiver(
                bank_notification_receiver,
                exit_updated_slot_server.clone(),
                slot_status_notifier,
            )),
            exit_updated_slot_server,
        }
    }

    pub fn join(&mut self) -> thread::Result<()> {
        self.exit_updated_slot_server.store(true, Ordering::Relaxed);
        self.bank_notification_receiver_service
            .take()
            .map(JoinHandle::join)
            .unwrap()
    }

    fn run_bank_notification_receiver(
        bank_notification_receiver: Receiver<SlotNotification>,
        exit: Arc<AtomicBool>,
        slot_status_notifier: SlotStatusNotifier,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("solBankNotif".to_string())
            .spawn(move || {
                while !exit.load(Ordering::Relaxed) {
                    if let Ok(slot) = bank_notification_receiver.recv() {
                        match slot {
                            SlotNotification::OptimisticallyConfirmed(slot) => {
                                slot_status_notifier
                                    .read()
                                    .unwrap()
                                    .notify_slot_confirmed(slot, None);
                            }
                            SlotNotification::Frozen((slot, parent)) => {
                                slot_status_notifier
                                    .read()
                                    .unwrap()
                                    .notify_slot_processed(slot, Some(parent));
                            }
                            SlotNotification::Root((slot, parent)) => {
                                slot_status_notifier
                                    .read()
                                    .unwrap()
                                    .notify_slot_rooted(slot, Some(parent));
                            }
                        }
                    }
                }
            })
            .unwrap()
    }
}
