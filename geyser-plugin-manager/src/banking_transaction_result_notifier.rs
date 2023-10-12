use {
    crate::geyser_plugin_manager::GeyserPluginManager,
    log::{error, log_enabled, trace},
    solana_measure::measure::Measure,
    solana_metrics::{create_counter, inc_counter, inc_new_counter, inc_new_counter_debug},
    solana_sdk::{
        slot_history::Slot,
        transaction::{SanitizedTransaction, TransactionError, TransactionResultNotifier},
    },
    std::sync::{Arc, RwLock},
};

#[derive(Debug)]
pub(crate) struct BankingTransactionResultImpl {
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
}

impl TransactionResultNotifier for BankingTransactionResultImpl {
    fn notify_banking_transaction_result(
        &self,
        transaction: &SanitizedTransaction,
        result: Option<TransactionError>,
        slot: Slot,
    ) {
        let mut measure = Measure::start("geyser-plugin-notify_plugins_of_entry_info");

        let plugin_manager = self.plugin_manager.read().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

        for plugin in plugin_manager.plugins.iter() {
            if !plugin.banking_transaction_results_notifications_enabled() {
                continue;
            }
            match plugin.notify_banking_stage_transaction_results(transaction, result.clone(), slot)
            {
                Err(err) => {
                    error!(
                        "Failed to notify banking transaction result, error: ({}) to plugin {}",
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully notified banking transaction result to plugin {}",
                        plugin.name()
                    );
                }
            }
        }
        measure.stop();
        inc_new_counter_debug!(
            "geyser-plugin-notify_plugins_of_banking_transaction_info-us",
            measure.as_us() as usize,
            10000,
            10000
        );
    }
}

impl BankingTransactionResultImpl {
    pub fn new(plugin_manager: Arc<RwLock<GeyserPluginManager>>) -> Self {
        Self { plugin_manager }
    }
}
