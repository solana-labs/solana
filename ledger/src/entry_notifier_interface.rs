use {solana_entry::entry::EntrySummary, solana_sdk::clock::Slot, std::sync::Arc};

pub trait EntryNotifier {
    fn notify_entry(&self, slot: Slot, index: usize, entry: &EntrySummary);
}

pub type EntryNotifierLock = Arc<dyn EntryNotifier + Sync + Send>;
