#[cfg(feature = "dev-context-only-utils")]
use qualifier_attr::qualifiers;
use {
    ahash::{AHashMap, AHashSet},
    solana_sdk::{pubkey::Pubkey, transaction::TransactionError},
    std::collections::hash_map,
};

#[derive(Debug, Default)]
pub struct AccountLocks {
    write_locks: AHashSet<Pubkey>,
    readonly_locks: AHashMap<Pubkey, u64>,
}

impl AccountLocks {
    /// Lock the account keys in `keys` for a transaction.
    /// The bool in the tuple indicates if the account is writable.
    /// Returns an error if any of the accounts are already locked in a way
    /// that conflicts with the requested lock.
    pub fn try_lock_accounts<'a>(
        &mut self,
        keys: impl Iterator<Item = (&'a Pubkey, bool)> + Clone,
    ) -> Result<(), TransactionError> {
        for (key, writable) in keys.clone() {
            if writable {
                if !self.can_write_lock(key) {
                    return Err(TransactionError::AccountInUse);
                }
            } else if !self.can_read_lock(key) {
                return Err(TransactionError::AccountInUse);
            }
        }

        for (key, writable) in keys {
            if writable {
                self.lock_write(key);
            } else {
                self.lock_readonly(key);
            }
        }

        Ok(())
    }

    /// Unlock the account keys in `keys` after a transaction.
    /// The bool in the tuple indicates if the account is writable.
    /// In debug-mode this function will panic if an attempt is made to unlock
    /// an account that wasn't locked in the way requested.
    pub fn unlock_accounts<'a>(&mut self, keys: impl Iterator<Item = (&'a Pubkey, bool)>) {
        for (k, writable) in keys {
            if writable {
                self.unlock_write(k);
            } else {
                self.unlock_readonly(k);
            }
        }
    }

    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
    fn is_locked_readonly(&self, key: &Pubkey) -> bool {
        self.readonly_locks
            .get(key)
            .map_or(false, |count| *count > 0)
    }

    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
    fn is_locked_write(&self, key: &Pubkey) -> bool {
        self.write_locks.contains(key)
    }

    fn can_read_lock(&self, key: &Pubkey) -> bool {
        // If the key is not write-locked, it can be read-locked
        !self.is_locked_write(key)
    }

    fn can_write_lock(&self, key: &Pubkey) -> bool {
        // If the key is not read-locked or write-locked, it can be write-locked
        !self.is_locked_readonly(key) && !self.is_locked_write(key)
    }

    fn lock_readonly(&mut self, key: &Pubkey) {
        *self.readonly_locks.entry(*key).or_default() += 1;
    }

    fn lock_write(&mut self, key: &Pubkey) {
        self.write_locks.insert(*key);
    }

    fn unlock_readonly(&mut self, key: &Pubkey) {
        if let hash_map::Entry::Occupied(mut occupied_entry) = self.readonly_locks.entry(*key) {
            let count = occupied_entry.get_mut();
            *count -= 1;
            if *count == 0 {
                occupied_entry.remove_entry();
            }
        } else {
            debug_assert!(
                false,
                "Attempted to remove a read-lock for a key that wasn't read-locked"
            );
        }
    }

    fn unlock_write(&mut self, key: &Pubkey) {
        let removed = self.write_locks.remove(key);
        debug_assert!(
            removed,
            "Attempted to remove a write-lock for a key that wasn't write-locked"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_locks() {
        let mut account_locks = AccountLocks::default();

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();

        // Add write and read-lock.
        let result = account_locks.try_lock_accounts([(&key1, true), (&key2, false)].into_iter());
        assert!(result.is_ok());

        // Try to add duplicate write-lock.
        let result = account_locks.try_lock_accounts([(&key1, true)].into_iter());
        assert_eq!(result, Err(TransactionError::AccountInUse));

        // Try to add write lock on read-locked account.
        let result = account_locks.try_lock_accounts([(&key2, true)].into_iter());
        assert_eq!(result, Err(TransactionError::AccountInUse));

        // Try to add read lock on write-locked account.
        let result = account_locks.try_lock_accounts([(&key1, false)].into_iter());
        assert_eq!(result, Err(TransactionError::AccountInUse));

        // Add read lock on read-locked account.
        let result = account_locks.try_lock_accounts([(&key2, false)].into_iter());
        assert!(result.is_ok());

        // Unlock write and read locks.
        account_locks.unlock_accounts([(&key1, true), (&key2, false)].into_iter());

        // No more remaining write-locks. Read-lock remains.
        assert!(!account_locks.is_locked_write(&key1));
        assert!(account_locks.is_locked_readonly(&key2));

        // Unlock read lock.
        account_locks.unlock_accounts([(&key2, false)].into_iter());
        assert!(!account_locks.is_locked_readonly(&key2));
    }
}
