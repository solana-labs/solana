
use {
    crossbeam_channel::Receiver,
    solana_accountsdb_plugin_intf::accountsdb_plugin_intf::SlotStatus,
    solana_rpc::optimistically_confirmed_bank_tracker::BankNotification,
    solana_runtime::accounts_db::AccountsUpdateNotifier,
    solana_sdk::{clock::Slot},
    std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::Duration,
    },
};

/// The structure modelling the slots eligible for replication and
/// their states.
#[derive(Default, Clone)]
struct EligibleSlotSet {
    slot_set: Arc<RwLock<VecDeque<(Slot, SlotStatus)>>>,
}

#[derive(Debug)]
pub(crate) struct SlotStatusObserver {
    confirmed_bank_receiver_service: Option<JoinHandle<()>>,
    plugin_notify_service: Option<JoinHandle<()>>,
    exit_updated_slot_server: Arc<AtomicBool>,
}

impl SlotStatusObserver {
    pub fn new(
        confirmed_bank_receiver: Receiver<BankNotification>,
        accounts_update_notifier: AccountsUpdateNotifier,
    ) -> Self {
        let eligible_slot_set = EligibleSlotSet::default();
        let exit_updated_slot_server = Arc::new(AtomicBool::new(false));

        Self {
            confirmed_bank_receiver_service: Some(Self::run_confirmed_bank_receiver(
                confirmed_bank_receiver,
                eligible_slot_set.clone(),
                exit_updated_slot_server.clone(),
            )),
            plugin_notify_service: Some(Self::run_plugin_notify_service(
                eligible_slot_set,
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
            .expect("confirmed_bank_receiver_service");

        self.plugin_notify_service
            .take()
            .map(JoinHandle::join)
            .unwrap()
    }

    fn run_confirmed_bank_receiver(
        confirmed_bank_receiver: Receiver<BankNotification>,
        eligible_slot_set: EligibleSlotSet,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("confirmed_bank_receiver".to_string())
            .spawn(move || {
                while !exit.load(Ordering::Relaxed) {
                    if let Ok(slot) = confirmed_bank_receiver.recv() {
                        let mut slot_set = eligible_slot_set.slot_set.write().unwrap();
                        match slot {
                            BankNotification::OptimisticallyConfirmed(slot) => {
                                slot_set.push_back((slot, SlotStatus::Confirmed));
                            }
                            BankNotification::Frozen(bank) => {
                                slot_set.push_back((bank.slot(), SlotStatus::Processed));
                            }
                            BankNotification::Root(bank) => {
                                slot_set.push_back((bank.slot(), SlotStatus::Rooted));
                            }
                        }
                    }
                }
            })
            .unwrap()
    }

    fn run_plugin_notify_service(
        eligible_slot_set: EligibleSlotSet,
        exit: Arc<AtomicBool>,
        accounts_update_notifier: AccountsUpdateNotifier,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("plugin_notify_service".to_string())
            .spawn(move || {
                while !exit.load(Ordering::Relaxed) {
                    let mut slot_set = eligible_slot_set.slot_set.write().unwrap();

                    loop {
                        let slot = slot_set.pop_front();
                        if slot.is_none() {
                            break;
                        }
                        accounts_update_notifier
                            .read()
                            .unwrap()
                            .notify_slot_confirmed(slot.unwrap().0);
                    }
                    drop(slot_set);
                    sleep(Duration::from_millis(200));
                }
            })
            .unwrap()
    }
}
