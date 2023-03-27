/// Module responsible for notifying plugins of transactions
use {
    crate::geyser_plugin_manager::GeyserPluginManager,
    log::*,
    solana_entry::entry::Entry,
    solana_geyser_plugin_interface::geyser_plugin_interface::{
        ReplicaEntryInfo, ReplicaEntryInfoVersions, ReplicaTransactionInfoV2,
        ReplicaTransactionInfoVersions,
    },
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_rpc::transaction_notifier_interface::TransactionNotifier,
    solana_sdk::{clock::Slot, signature::Signature, transaction::SanitizedTransaction},
    solana_transaction_status::TransactionStatusMeta,
    std::sync::{Arc, RwLock},
};

/// This implementation of TransactionNotifier is passed to the rpc's TransactionStatusService
/// at the validator startup. TransactionStatusService invokes the notify_transaction method
/// for new transactions. The implementation in turn invokes the notify_transaction of each
/// plugin enabled with transaction notification managed by the GeyserPluginManager.
pub(crate) struct TransactionNotifierImpl {
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
}

impl TransactionNotifier for TransactionNotifierImpl {
    fn notify_transaction(
        &self,
        slot: Slot,
        index: usize,
        signature: &Signature,
        transaction_status_meta: &TransactionStatusMeta,
        transaction: &SanitizedTransaction,
    ) {
        let mut measure = Measure::start("geyser-plugin-notify_plugins_of_transaction_info");
        let transaction_log_info = Self::build_replica_transaction_info(
            index,
            signature,
            transaction_status_meta,
            transaction,
        );

        let plugin_manager = self.plugin_manager.read().unwrap();

        if plugin_manager.plugins.is_empty() {
            return;
        }

        for plugin in plugin_manager.plugins.iter() {
            if !plugin.transaction_notifications_enabled() {
                continue;
            }
            match plugin.notify_transaction(
                ReplicaTransactionInfoVersions::V0_0_2(&transaction_log_info),
                slot,
            ) {
                Err(err) => {
                    error!(
                        "Failed to notify transaction, error: ({}) to plugin {}",
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully notified transaction to plugin {}",
                        plugin.name()
                    );
                }
            }
        }
        measure.stop();
        inc_new_counter_debug!(
            "geyser-plugin-notify_plugins_of_transaction_info-us",
            measure.as_us() as usize,
            10000,
            10000
        );
    }

    fn notify_entry<'a>(&'a self, slot: Slot, index: usize, entry: &'a Entry) {
        let mut measure = Measure::start("geyser-plugin-notify_plugins_of_entry_info");

        let plugin_manager = self.plugin_manager.read().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

        let entry_info = Self::build_replica_entry_info(slot, index, entry);

        for plugin in plugin_manager.plugins.iter() {
            if !plugin.entry_notifications_enabled() {
                continue;
            }
            match plugin.notify_entry(ReplicaEntryInfoVersions::V0_0_1(&entry_info)) {
                Err(err) => {
                    error!(
                        "Failed to notify entry, error: ({}) to plugin {}",
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!("Successfully notified entry to plugin {}", plugin.name());
                }
            }
        }
        measure.stop();
        inc_new_counter_debug!(
            "geyser-plugin-notify_plugins_of_entry_info-us",
            measure.as_us() as usize,
            10000,
            10000
        );
    }
}

impl TransactionNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<GeyserPluginManager>>) -> Self {
        Self { plugin_manager }
    }

    fn build_replica_transaction_info<'a>(
        index: usize,
        signature: &'a Signature,
        transaction_status_meta: &'a TransactionStatusMeta,
        transaction: &'a SanitizedTransaction,
    ) -> ReplicaTransactionInfoV2<'a> {
        ReplicaTransactionInfoV2 {
            index,
            signature,
            is_vote: transaction.is_simple_vote_transaction(),
            transaction,
            transaction_status_meta,
        }
    }

    fn build_replica_entry_info(
        slot: Slot,
        index: usize,
        entry: &'_ Entry,
    ) -> ReplicaEntryInfo<'_> {
        ReplicaEntryInfo {
            slot,
            index,
            num_hashes: entry.num_hashes,
            hash: entry.hash.as_ref(),
            executed_transaction_count: entry.transactions.len() as u64,
        }
    }
}
