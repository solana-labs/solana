use {
    crate::rpc_subscriptions::RpcSubscriptions,
    solana_client::rpc_response::{SlotShredStats, SlotUpdate},
    solana_ledger::blockstore::CompletedSlotsReceiver,
    solana_sdk::timing::timestamp,
    std::{
        sync::Arc,
        thread::{Builder, JoinHandle},
    },
};

pub struct RpcCompletedSlotsService;
impl RpcCompletedSlotsService {
    pub fn spawn(
        completed_slots_receiver: CompletedSlotsReceiver,
        rpc_subscriptions: Arc<RpcSubscriptions>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("solana-rpc-completed-slots-service".to_string())
            .spawn(move || {
                for slots_and_stats in completed_slots_receiver.iter() {
                    for (slot, slot_stats) in slots_and_stats {
                        let stats = slot_stats.map(|stats| SlotShredStats {
                            num_shreds: stats.num_shreds as u64,
                            num_repaired: stats.num_repaired as u64,
                            num_recovered: stats.num_recovered as u64,
                            turbine_indices: stats.turbine_index_set.iter().map(|ix| *ix).collect(),
                        });
                        rpc_subscriptions.notify_slot_update(SlotUpdate::Completed {
                            slot,
                            timestamp: timestamp(),
                            stats,
                        });
                    }
                }
            })
            .unwrap()
    }
}
