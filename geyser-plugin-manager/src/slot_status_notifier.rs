use {
    crate::geyser_plugin_manager::GeyserPluginManager,
    log::*,
    solana_geyser_plugin_interface::geyser_plugin_interface::SlotStatus,
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_runtime::bank::Bank,
    solana_sdk::clock::Slot,
    std::sync::{Arc, RwLock},
};

pub trait SlotStatusNotifierInterface {
    /// Notified when a slot is optimistically confirmed
    fn notify_slot_confirmed(&self, slot: Slot, bank: Option<Arc<Bank>>);

    /// Notified when a slot is marked frozen.
    fn notify_slot_processed(&self, slot: Slot, bank: Option<Arc<Bank>>);

    /// Notified when a slot is rooted.
    fn notify_slot_rooted(&self, slot: Slot, bank: Option<Arc<Bank>>);
}

pub type SlotStatusNotifier = Arc<RwLock<dyn SlotStatusNotifierInterface + Sync + Send>>;

pub struct SlotStatusNotifierImpl {
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
}

impl SlotStatusNotifierInterface for SlotStatusNotifierImpl {
    fn notify_slot_confirmed(&self, slot: Slot, bank: Option<Arc<Bank>>) {
        self.notify_slot_status(slot, bank, SlotStatus::Confirmed);
    }

    fn notify_slot_processed(&self, slot: Slot, bank: Option<Arc<Bank>>) {
        self.notify_slot_status(slot, bank, SlotStatus::Processed);
    }

    fn notify_slot_rooted(&self, slot: Slot, bank: Option<Arc<Bank>>) {
        self.notify_slot_status(slot, bank, SlotStatus::Rooted);
    }
}

impl SlotStatusNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<GeyserPluginManager>>) -> Self {
        Self { plugin_manager }
    }

    pub fn notify_slot_status(&self, slot: Slot, bank: Option<Arc<Bank>>, slot_status: SlotStatus) {
        let plugin_manager = self.plugin_manager.read().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

        let parent = bank.as_ref().map(|bank| bank.parent_slot());
        for plugin in plugin_manager.plugins.iter() {
            let mut measure = Measure::start("geyser-plugin-update-slot");
            match plugin.update_slot_status(slot, parent, slot_status) {
                Err(err) => {
                    error!(
                        "Failed to update slot status at slot {}, error: {} to plugin {}",
                        slot,
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated slot status at slot {} to plugin {}",
                        slot,
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "geyser-plugin-update-slot-us",
                measure.as_us() as usize,
                1000,
                1000
            );

            let mut measure = Measure::start("geyser-plugin-update-bank");
            match plugin.update_bank(bank.clone(), slot_status) {
                Err(err) => {
                    error!(
                        "Failed to update bank at slot {}, error: {} to plugin {}",
                        slot,
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated bank at slot {} to plugin {}",
                        slot,
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "geyser-plugin-update-bank-us",
                measure.as_us() as usize,
                1000,
                1000
            );
        }
    }
}
