/// Module responsible for notifying plugins of transactions
use solana_gossip::legacy_contact_info::LegacyContactInfo;
use solana_sdk::pubkey::Pubkey;
use {
    crate::geyser_plugin_manager::GeyserPluginManager,
    log::*,
    solana_client::connection_cache::Protocol,
    solana_geyser_plugin_interface::geyser_plugin_interface::ReplicaClusterInfoNode,
    solana_gossip::cluster_info_notifier_interface::ClusterInfoNotifierInterface,
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_rpc::transaction_notifier_interface::TransactionNotifier,
    solana_sdk::{clock::Slot, signature::Signature, transaction::SanitizedTransaction},
    solana_transaction_status::TransactionStatusMeta,
    std::sync::{Arc, RwLock},
};

/// This implementation of ClusterInfoNotifierImpl is passed to the rpc's TransactionStatusService
/// at the validator startup. TransactionStatusService invokes the notify_transaction method
/// for new transactions. The implementation in turn invokes the notify_transaction of each
/// plugin enabled with transaction notification managed by the GeyserPluginManager.
#[derive(Debug)]
pub(crate) struct ClusterInfoNotifierImpl {
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
}

impl ClusterInfoNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<GeyserPluginManager>>) -> Self {
        ClusterInfoNotifierImpl { plugin_manager }
    }

    fn clusterinfo_from_legacy_contact_info(
        legacy_info: &LegacyContactInfo,
    ) -> ReplicaClusterInfoNode {
        ReplicaClusterInfoNode {
            id: *legacy_info.pubkey(),
            /// gossip address
            gossip: legacy_info.gossip().ok(),
            /// address to connect to for replication
            tvu: legacy_info.tvu(Protocol::UDP).ok(),
            /// TVU over QUIC protocol.
            tvu_quic: legacy_info.tvu(Protocol::QUIC).ok(),
            /// repair service over QUIC protocol.
            serve_repair_quic: legacy_info.serve_repair(Protocol::QUIC).ok(),
            /// transactions address
            tpu: legacy_info.tpu(Protocol::UDP).ok(),
            /// address to forward unprocessed transactions to
            tpu_forwards: legacy_info.tpu_forwards(Protocol::UDP).ok(),
            /// address to which to send bank state requests
            tpu_vote: legacy_info.tpu_vote().ok(),
            /// address to which to send JSON-RPC requests
            rpc: legacy_info.rpc().ok(),
            /// websocket for JSON-RPC push notifications
            rpc_pubsub: legacy_info.rpc_pubsub().ok(),
            /// address to send repair requests to
            serve_repair: legacy_info.serve_repair(Protocol::UDP).ok(),
            /// latest wallclock picked
            wallclock: legacy_info.wallclock(),
            /// node shred version
            shred_version: legacy_info.shred_version(),
        }
    }
}

impl ClusterInfoNotifierInterface for ClusterInfoNotifierImpl {
    fn notify_clusterinfo_update(&self, contact_info: &LegacyContactInfo) {
        let cluster_info =
            ClusterInfoNotifierImpl::clusterinfo_from_legacy_contact_info(contact_info);
        let mut measure2 = Measure::start("geyser-plugin-notify_plugins_of_cluster_info_update");
        let plugin_manager = self.plugin_manager.read().unwrap();

        if plugin_manager.plugins.is_empty() {
            return;
        }
        for plugin in plugin_manager.plugins.iter() {
            let mut measure = Measure::start("geyser-plugin-update-cluster_info");
            match plugin.update_cluster_info(&cluster_info) {
                Err(err) => {
                    error!(
                        "Failed to update cluster_info {}, error: {} to plugin {}",
                        bs58::encode(cluster_info.id).into_string(),
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated cluster_info {} to plugin {}",
                        bs58::encode(cluster_info.id).into_string(),
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "geyser-plugin-update-cluster_info-us",
                measure.as_us() as usize,
                100000,
                100000
            );
        }
        measure2.stop();
        inc_new_counter_debug!(
            "geyser-plugin-notify_plugins_of_cluster_info_update-us",
            measure2.as_us() as usize,
            100000,
            100000
        );
    }

    fn notify_clusterinfo_remove(&self, pubkey: &Pubkey) {
        let mut measure2 = Measure::start("geyser-plugin-notify_plugins_of_cluster_info_update");
        let plugin_manager = self.plugin_manager.read().unwrap();

        if plugin_manager.plugins.is_empty() {
            return;
        }
        for plugin in plugin_manager.plugins.iter() {
            let mut measure = Measure::start("geyser-plugin-remove-cluster_info");
            match plugin.notify_clusterinfo_remove(pubkey) {
                Err(err) => {
                    error!(
                        "Failed to remove cluster_info {}, error: {} to plugin {}",
                        bs58::encode(pubkey).into_string(),
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully remove cluster_info {} to plugin {}",
                        bs58::encode(pubkey).into_string(),
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "geyser-plugin-remove-cluster_info-us",
                measure.as_us() as usize,
                100000,
                100000
            );
        }
        measure2.stop();
        inc_new_counter_debug!(
            "geyser-plugin-notify_plugins_of_cluster_info_remove-us",
            measure2.as_us() as usize,
            100000,
            100000
        );
    }
}
