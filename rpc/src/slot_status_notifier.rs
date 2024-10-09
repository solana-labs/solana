use {
    solana_sdk::clock::Slot,
    std::sync::{Arc, RwLock},
};

pub trait SlotStatusNotifierInterface {
    /// Notified when a slot is optimistically confirmed
    fn notify_slot_confirmed(&self, slot: Slot, parent: Option<Slot>);

    /// Notified when a slot is marked frozen.
    fn notify_slot_processed(&self, slot: Slot, parent: Option<Slot>);

    /// Notified when a slot is rooted.
    fn notify_slot_rooted(&self, slot: Slot, parent: Option<Slot>);

    /// Notified when the first shred is received for a slot.
    fn notify_first_shred_received(&self, slot: Slot);

    /// Notified when the slot is completed.
    fn notify_completed(&self, slot: Slot);
}

pub type SlotStatusNotifier = Arc<RwLock<dyn SlotStatusNotifierInterface + Sync + Send>>;
