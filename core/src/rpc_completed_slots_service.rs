use crate::rpc_subscriptions::RpcSubscriptions;
use solana_client::rpc_response::SlotUpdate;
use solana_ledger::blockstore::CompletedSlotsReceiver;
use solana_sdk::timing::timestamp;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::RecvTimeoutError,
        Arc,
    },
    thread::{Builder, JoinHandle},
    time::Duration,
};

pub struct RpcCompletedSlotsService;
impl RpcCompletedSlotsService {
    pub fn spawn(
        completed_slots_receiver: CompletedSlotsReceiver,
        rpc_subscriptions: Option<Arc<RpcSubscriptions>>,
        exit: Arc<AtomicBool>,
    ) -> Option<JoinHandle<()>> {
        let rpc_subscriptions = rpc_subscriptions?;
        Some(Builder::new()
            .name("solana-rpc-completed-slots-service".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                let mut slots = match completed_slots_receiver.recv_timeout(Duration::from_millis(200))
                {
                    Ok(slots) => slots,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => {
                        warn!("RPC completed slots service - sender disconnected");
                        break;
                    }
                };

                while let Ok(mut more) = completed_slots_receiver.try_recv() {
                    slots.append(&mut more);
                }
                #[allow(clippy::stable_sort_primitive)]
                slots.sort();

                for slot in slots {
                    rpc_subscriptions.notify_slot_update(SlotUpdate::ShredsFull {
                        slot,
                        timestamp: timestamp(),
                    });
                }
            })
            .unwrap())
    }
}
