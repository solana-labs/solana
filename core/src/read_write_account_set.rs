use {
    solana_sdk::{
        message::{SanitizedMessage, VersionedMessage},
        pubkey::Pubkey,
    },
    std::collections::HashSet,
};

/// Wrapper struct to check account locks for a batch of transactions.
#[derive(Debug, Default)]
pub struct ReadWriteAccountSet {
    /// Set of accounts that are locked for read
    read_set: HashSet<Pubkey>,
    /// Set of accounts that are locked for write
    write_set: HashSet<Pubkey>,
}

impl ReadWriteAccountSet {
    /// Check static account locks for a transaction message.
    pub fn check_static_account_locks(&self, message: &VersionedMessage) -> bool {
        !message
            .static_account_keys()
            .iter()
            .enumerate()
            .any(|(index, pubkey)| {
                if message.is_maybe_writable(index) {
                    !self.can_write(pubkey)
                } else {
                    !self.can_read(pubkey)
                }
            })
    }

    /// Check all account locks and if they are available, lock them.
    /// Returns true if all account locks are available and false otherwise.
    pub fn try_locking(&mut self, message: &SanitizedMessage) -> bool {
        if self.check_sanitized_message_account_locks(message) {
            message
                .account_keys()
                .iter()
                .enumerate()
                .for_each(|(index, pubkey)| {
                    if message.is_writable(index) {
                        self.add_write(pubkey);
                    } else {
                        self.add_read(pubkey);
                    }
                });
            true
        } else {
            false
        }
    }

    /// Check if a sanitized message's account locks are available.
    fn check_sanitized_message_account_locks(&self, message: &SanitizedMessage) -> bool {
        !message
            .account_keys()
            .iter()
            .enumerate()
            .any(|(index, pubkey)| {
                if message.is_writable(index) {
                    !self.can_write(pubkey)
                } else {
                    !self.can_read(pubkey)
                }
            })
    }

    /// Check if an account can be read-locked
    fn can_read(&self, pubkey: &Pubkey) -> bool {
        !self.write_set.contains(pubkey)
    }

    /// Check if an account can be write-locked
    fn can_write(&self, pubkey: &Pubkey) -> bool {
        !self.read_set.contains(pubkey) && !self.write_set.contains(pubkey)
    }

    /// Add an account to the read-set.
    /// Should only be called after `can_read()` returns true
    fn add_read(&mut self, pubkey: &Pubkey) {
        self.read_set.insert(*pubkey);
    }

    /// Add an account to the write-set.
    /// Should only be called after `can_write()` returns true
    fn add_write(&mut self, pubkey: &Pubkey) {
        assert!(self.write_set.insert(*pubkey), "Write lock already held");
    }

    /// Clears the read and write sets
    pub fn clear(&mut self) {
        self.read_set.clear();
        self.write_set.clear();
    }
}

#[cfg(test)]
mod tests {
    use {super::ReadWriteAccountSet, solana_sdk::pubkey::Pubkey};

    #[test]
    pub fn test_write_write_conflict() {
        let mut account_locks = ReadWriteAccountSet::default();
        let account = Pubkey::new_unique();
        assert!(account_locks.can_write(&account));
        account_locks.add_write(&account);
        assert!(!account_locks.can_write(&account));
    }

    #[test]
    pub fn test_read_write_conflict() {
        let mut account_locks = ReadWriteAccountSet::default();
        let account = Pubkey::new_unique();
        assert!(account_locks.can_read(&account));
        account_locks.add_read(&account);
        assert!(!account_locks.can_write(&account));
        assert!(account_locks.can_read(&account));
    }

    #[test]
    pub fn test_write_read_conflict() {
        let mut account_locks = ReadWriteAccountSet::default();
        let account = Pubkey::new_unique();
        assert!(account_locks.can_write(&account));
        account_locks.add_write(&account);
        assert!(!account_locks.can_write(&account));
        assert!(!account_locks.can_read(&account));
    }

    #[test]
    pub fn test_write_write_non_conflict() {
        let mut account_locks = ReadWriteAccountSet::default();
        let account1 = Pubkey::new_unique();
        let account2 = Pubkey::new_unique();
        assert!(account_locks.can_write(&account1));
        account_locks.add_write(&account1);
        assert!(account_locks.can_write(&account2));
        assert!(account_locks.can_read(&account2));
    }
}
