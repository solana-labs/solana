use {
    crossbeam_channel::Receiver,
    solana_rpc::optimistically_confirmed_bank_tracker::BankNotification,
    solana_runtime::accounts_update_notifier_interface::AccountsUpdateNotifier,
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
    confirmed_bank_receiver_service: Option<JoinHandle<()>>,
    exit_updated_slot_server: Arc<AtomicBool>,
}

impl SlotStatusObserver {
    pub fn new(
        confirmed_bank_receiver: Receiver<BankNotification>,
        accounts_update_notifier: AccountsUpdateNotifier,
    ) -> Self {
        let exit_updated_slot_server = Arc::new(AtomicBool::new(false));

        Self {
            confirmed_bank_receiver_service: Some(Self::run_confirmed_bank_receiver(
                confirmed_bank_receiver,
                exit_updated_slot_server.clone(),
                accounts_update_notifier,
            )),
            exit_updated_slot_server,
        }
    }

    pub fn join(&mut self) -> thread::Result<()> {
        self.exit_updated_slot_server.store(true, Ordering::Relaxed);
        self.confirmed_bank_receiver_service
            .take()
            .map(JoinHandle::join)
            .unwrap()
    }

    fn run_confirmed_bank_receiver(
        confirmed_bank_receiver: Receiver<BankNotification>,
        exit: Arc<AtomicBool>,
        accounts_update_notifier: AccountsUpdateNotifier,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("confirmed_bank_receiver".to_string())
            .spawn(move || {
                while !exit.load(Ordering::Relaxed) {
                    if let Ok(slot) = confirmed_bank_receiver.recv() {
                        match slot {
                            BankNotification::OptimisticallyConfirmed(slot) => {
                                accounts_update_notifier
                                    .read()
                                    .unwrap()
                                    .notify_slot_confirmed(slot, None);
                            }
                            BankNotification::Frozen(bank) => {
                                accounts_update_notifier
                                    .read()
                                    .unwrap()
                                    .notify_slot_processed(bank.slot(), Some(bank.parent_slot()));
                            }
                            BankNotification::Root(bank) => {
                                accounts_update_notifier
                                    .read()
                                    .unwrap()
                                    .notify_slot_rooted(bank.slot(), Some(bank.parent_slot()));
                            }
                        }
                    }
                }
            })
            .unwrap()
    }
}
