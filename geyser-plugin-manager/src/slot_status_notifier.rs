use {
    crate::geyser_plugin_manager::GeyserPluginManager,
    agave_geyser_plugin_interface::geyser_plugin_interface::SlotStatus,
    log::*,
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_rpc::slot_status_notifier::SlotStatusNotifierInterface,
    solana_sdk::clock::Slot,
    std::sync::{Arc, RwLock},
};

pub struct SlotStatusNotifierImpl {
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
}

impl SlotStatusNotifierInterface for SlotStatusNotifierImpl {
    fn notify_slot_confirmed(&self, slot: Slot, parent: Option<Slot>) {
        self.notify_slot_status(slot, parent, SlotStatus::Confirmed);
    }

    fn notify_slot_processed(&self, slot: Slot, parent: Option<Slot>) {
        self.notify_slot_status(slot, parent, SlotStatus::Processed);
    }

    fn notify_slot_rooted(&self, slot: Slot, parent: Option<Slot>) {
        self.notify_slot_status(slot, parent, SlotStatus::Rooted);
    }

    fn notify_first_shred_received(&self, slot: Slot) {
        self.notify_slot_status(slot, None, SlotStatus::FirstShredReceived);
    }

    fn notify_completed(&self, slot: Slot) {
        self.notify_slot_status(slot, None, SlotStatus::Completed);
    }

    fn notify_created_bank(&self, slot: Slot, parent: Slot) {
        self.notify_slot_status(slot, Some(parent), SlotStatus::CreatedBank);
    }
}

impl SlotStatusNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<GeyserPluginManager>>) -> Self {
        Self { plugin_manager }
    }

    pub fn notify_slot_status(&self, slot: Slot, parent: Option<Slot>, slot_status: SlotStatus) {
        let plugin_manager = self.plugin_manager.read().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

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
        }
    }
}
