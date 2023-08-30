/// Module responsible for notifying plugins about entries
use {
    crate::geyser_plugin_manager::GeyserPluginManager,
    log::*,
    solana_entry::entry::EntrySummary,
    solana_geyser_plugin_interface::geyser_plugin_interface::{
        ReplicaEntryInfo, ReplicaEntryInfoVersions,
    },
    solana_ledger::entry_notifier_interface::EntryNotifier,
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_sdk::clock::Slot,
    std::sync::{Arc, RwLock},
};

pub(crate) struct EntryNotifierImpl {
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
}

impl EntryNotifier for EntryNotifierImpl {
    fn notify_entry<'a>(&'a self, slot: Slot, index: usize, entry: &'a EntrySummary) {
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

impl EntryNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<GeyserPluginManager>>) -> Self {
        Self { plugin_manager }
    }

    fn build_replica_entry_info(
        slot: Slot,
        index: usize,
        entry: &'_ EntrySummary,
    ) -> ReplicaEntryInfo<'_> {
        ReplicaEntryInfo {
            slot,
            index,
            num_hashes: entry.num_hashes,
            hash: entry.hash.as_ref(),
            executed_transaction_count: entry.num_transactions,
        }
    }
}
