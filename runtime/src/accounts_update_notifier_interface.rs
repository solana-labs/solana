use {
    crate::append_vec::StoredAccountMeta,
    solana_sdk::{account::AccountSharedData, clock::Slot, pubkey::Pubkey},
    std::sync::{Arc, RwLock},
};

pub trait AccountsUpdateNotifierInterface: std::fmt::Debug {
    /// Notified when an account is updated at runtime, due to transaction activities
    fn notify_account_update(&self, slot: Slot, pubkey: &Pubkey, account: &AccountSharedData);

    /// Notified when the AccountsDb is initialized at start when restored
    /// from a snapshot.
    fn notify_account_restore_from_snapshot(&self, slot: Slot, account: &StoredAccountMeta);

    /// Notified when a slot is optimistically confirmed
    fn notify_slot_confirmed(&self, slot: Slot, parent: Option<Slot>);

    /// Notified when a slot is marked frozen.
    fn notify_slot_processed(&self, slot: Slot, parent: Option<Slot>);

    /// Notified when a slot is rooted.
    fn notify_slot_rooted(&self, slot: Slot, parent: Option<Slot>);
}

pub type AccountsUpdateNotifier = Arc<RwLock<dyn AccountsUpdateNotifierInterface + Sync + Send>>;
