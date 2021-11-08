/// Module responsible for notifying plugins of transactions
use {
    crate::accountsdb_plugin_manager::AccountsDbPluginManager,
    log::*,
    solana_accountsdb_plugin_interface::accountsdb_plugin_interface::{
        ReplicaTransactionLogInfo, ReplicaTranscaionLogInfoVersions,
    },
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_rpc::transaction_notifier_interface::TransactionNotifierInterface,
    solana_runtime::bank,
    solana_sdk::{
        clock::Slot, signature::Signature, transaction::SanitizedTransaction,
    },
    solana_transaction_status::TransactionStatusMeta,
    std::sync::{Arc, RwLock},
};

pub(crate) struct TransactionNotifierImpl {
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
}

impl TransactionNotifierInterface for TransactionNotifierImpl {
    fn notify_transaction(
        &self,
        slot: Slot,
        signature: &Signature,
        status: &TransactionStatusMeta,
        transaction: &SanitizedTransaction,
    ) {
        self.notify_transaction_log_info(
            slot,
            signature,
            status,
            transaction,
        );
    }
}

impl TransactionNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<AccountsDbPluginManager>>) -> Self {
        Self { plugin_manager }
    }

    fn build_replica_transaction_log_info<'a>(
        signature: &'a Signature,
        transaction_meta: &'a TransactionStatusMeta,
        transaction: &'a SanitizedTransaction,
    ) -> ReplicaTransactionLogInfo<'a> {
        ReplicaTransactionLogInfo {
            signature,
            is_vote: bank::is_simple_vote_transaction(transaction),
            transaction,
            transaction_meta,
        }
    }

    fn notify_transaction_log_info(
        &self,
        slot: Slot,
        signature: &Signature,
        transaction_meta: &TransactionStatusMeta,
        transaction: &SanitizedTransaction,
    ) {
        let mut measure =
            Measure::start("accountsdb-plugin-notify_plugins_of_transaction_log_info");
        let mut plugin_manager = self.plugin_manager.write().unwrap();

        if plugin_manager.plugins.is_empty() {
            return;
        }

        let transaction_log_info =
            Self::build_replica_transaction_log_info(signature, transaction_meta, &transaction);
        for plugin in plugin_manager.plugins.iter_mut() {
            match plugin.notify_transaction(
                ReplicaTranscaionLogInfoVersions::V0_0_1(&transaction_log_info),
                slot,
            ) {
                Err(err) => {
                    error!(
                        "Failed to notify transaction, error: {} to plugin {}",
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
            "accountsdb-plugin-notify_plugins_of_transaction_log_info-us",
            measure.as_us() as usize,
            10000,
            10000
        );
    }
}
