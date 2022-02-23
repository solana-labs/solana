//! trait for abstracting underlying storage of pubkey and account pairs to be written
use solana_sdk::{account::ReadableAccount, clock::Slot, pubkey::Pubkey};

/// abstract access to pubkey, account, slot, target_slot of either:
/// a. (slot, &[&Pubkey, &ReadableAccount])
/// b. (slot, &[&Pubkey, &ReadableAccount, Slot]) (we will use this later)
/// This trait avoids having to allocate redundant data when there is a duplicated slot parameter.
/// All legacy callers do not have a unique slot per account to store.
pub trait StorableAccounts<'a, T: ReadableAccount + Sync>: Sync {
    /// pubkey at 'index'
    fn pubkey(&self, index: usize) -> &Pubkey;
    /// account at 'index'
    fn account(&self, index: usize) -> &T;
    /// slot that all accounts are to be written to
    fn target_slot(&self) -> Slot;
    /// true if no accounts to write
    fn is_empty(&self) -> bool;
    /// # accounts to write
    fn len(&self) -> usize;
}

impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a, T> for (Slot, &'a [(&'a Pubkey, &'a T)]) {
    fn pubkey(&self, index: usize) -> &Pubkey {
        self.1[index].0
    }
    fn account(&self, index: usize) -> &T {
        self.1[index].1
    }
    fn target_slot(&self) -> Slot {
        self.0
    }
    fn is_empty(&self) -> bool {
        self.1.is_empty()
    }
    fn len(&self) -> usize {
        self.1.len()
    }
}
