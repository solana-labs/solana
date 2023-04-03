use {
    solana_entry::entry::Entry,
    solana_sdk::clock::Slot,
    std::sync::{Arc, RwLock},
};

pub trait EntryNotifier {
    fn notify_entry(&self, slot: Slot, index: usize, entry: &Entry);
}

pub type EntryNotifierLock = Arc<RwLock<dyn EntryNotifier + Sync + Send>>;
