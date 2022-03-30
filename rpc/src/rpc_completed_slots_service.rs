use {
    crate::rpc_subscriptions::RpcSubscriptions,
    solana_client::rpc_response::SlotUpdate,
    solana_ledger::blockstore::CompletedSlotsReceiver,
    solana_sdk::timing::timestamp,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{sleep, Builder, JoinHandle},
        time::Duration,
    },
};

pub const COMPLETE_SLOT_REPORT_SLEEP_MS: u64 = 100;

pub struct RpcCompletedSlotsService;
impl RpcCompletedSlotsService {
    pub fn spawn(
        completed_slots_receiver: CompletedSlotsReceiver,
        rpc_subscriptions: Arc<RpcSubscriptions>,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("solana-rpc-completed-slots-service".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }

                completed_slots_receiver.try_iter().for_each(|slots| {
                    for slot in slots {
                        rpc_subscriptions.notify_slot_update(SlotUpdate::Completed {
                            slot,
                            timestamp: timestamp(),
                        });
                    }
                });

                sleep(Duration::from_millis(COMPLETE_SLOT_REPORT_SLEEP_MS));
            })
            .unwrap()
    }
}
