use {
    super::{
        account_lock::AccountLock,
        thread_set::{ThreadId, ThreadSet, MAX_THREADS},
    },
    solana_sdk::pubkey::Pubkey,
    std::collections::HashMap,
};

/// Thread-aware account locks which allows for scheduling on threads
/// that already hold locks on the account. This is useful for allowing
/// queued transactions to be scheduled on a thread while the transaction
/// is still being executed on the thread, up to a queue limit.
pub(crate) struct ThreadAwareAccountLocks {
    /// Number of threads.
    num_threads: usize, // 0..MAX_THREADS
    /// Limit on the number of sequentially queued transactions per account.
    sequential_queue_limit: u32,
    /// Locks held for each account.
    locks: HashMap<Pubkey, AccountLock>,
}

impl ThreadAwareAccountLocks {
    /// Creates a new `ThreadAwareAccountLocks` with the given number of threads
    /// and queue limit.
    pub(crate) fn new(num_threads: usize, sequential_queue_limit: u32) -> Self {
        assert!(num_threads > 0 && num_threads <= MAX_THREADS);
        Self {
            num_threads,
            sequential_queue_limit,
            locks: HashMap::new(),
        }
    }

    /// Returns `ThreadSet` that the given accounts can be scheduled on.
    pub(crate) fn accounts_schedulable_threads<'a>(
        &self,
        write_account_locks: impl Iterator<Item = &'a Pubkey>,
        read_account_locks: impl Iterator<Item = &'a Pubkey>,
    ) -> ThreadSet {
        let mut schedulable_threads = ThreadSet::any(self.num_threads);

        for account_lock in write_account_locks.filter_map(|account| self.locks.get(account)) {
            schedulable_threads &= account_lock.write_schedulable(self.sequential_queue_limit);
        }

        for account_lock in read_account_locks.filter_map(|account| self.locks.get(account)) {
            schedulable_threads &=
                account_lock.read_schedulable(self.num_threads, self.sequential_queue_limit);
        }

        schedulable_threads
    }

    /// Add locks for all writable and readable accounts on `thread_id`.
    pub(crate) fn lock_accounts<'a>(
        &mut self,
        write_account_locks: impl Iterator<Item = &'a Pubkey>,
        read_account_locks: impl Iterator<Item = &'a Pubkey>,
        thread_id: ThreadId,
    ) {
        for account in write_account_locks {
            self.lock_write_account(account, thread_id);
        }

        for account in read_account_locks {
            self.lock_read_account(account, thread_id);
        }
    }

    /// Locks the given `account` for writing on `thread_id`.
    /// Panics if the account is already locked for writing on another thread.
    fn lock_write_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                entry.into_mut().lock_write(thread_id);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(AccountLock::new_write_lock(thread_id, 1));
            }
        }
    }

    /// Unlocks the given `account` for writing on `thread_id`.
    /// Panics if the account is not locked for writing on `thread_id`.
    fn unlock_write_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                entry.into_mut().unlock_write(thread_id);
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                panic!("No locks not found for account: {account}");
            }
        }
    }

    /// Locks the given `account` for reading on `thread_id`.
    /// Panics if the account is already locked for writing on another thread.
    fn lock_read_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                entry.into_mut().lock_read(thread_id);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(AccountLock::new_read_lock(thread_id, 1));
            }
        }
    }

    /// Unlocks the given `account` for reading on `thread_id`.
    /// Panics if the account is not locked for reading on `thread_id`.
    fn unlock_read_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                entry.into_mut().unlock_read(thread_id);
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                panic!("No locks not found for account: {account}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_NUM_THREADS: usize = 4;

    #[test]
    #[should_panic]
    fn too_few_num_threads() {
        ThreadAwareAccountLocks::new(0, 1);
    }

    #[test]
    #[should_panic]
    fn too_many_num_threads() {
        ThreadAwareAccountLocks::new(MAX_THREADS + 1, 1);
    }

    #[test]
    fn accounts_schedulable_threads() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);

        // No locks - all threads are schedulable
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::any(TEST_NUM_THREADS)
        );

        // Write lock on pk1 on thread 0 - now only thread 0 is schedulable
        locks.lock_write_account(&pk1, 0);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::only(0)
        );

        // Write lock pk2 on thread 0 - can still schedule on thread 0
        locks.lock_write_account(&pk2, 0);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::only(0)
        );

        // Move pk2 lock to thread 1 - cannot schedule on any threads
        locks.unlock_write_account(&pk2, 0);
        locks.lock_write_account(&pk2, 1);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::none()
        );

        // Remove pj2 lock, add another lock for pk1 on thread 0 - at `write_queue_limit` so cannot schedule on any threads
        locks.unlock_write_account(&pk2, 1);
        locks.lock_write_account(&pk1, 0);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::none()
        );
    }

    #[test]
    #[should_panic]
    fn test_lock_write_account_write_conflict_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.lock_write_account(&pk1, 0);
        locks.lock_write_account(&pk1, 1);
    }

    #[test]
    #[should_panic]
    fn test_lock_write_account_read_conflict_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.lock_read_account(&pk1, 0);
        locks.lock_write_account(&pk1, 1);
    }

    #[test]
    #[should_panic]
    fn test_unlock_write_account_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.lock_write_account(&pk1, 1);
        locks.unlock_write_account(&pk1, 0);
    }

    #[test]
    #[should_panic]
    fn test_lock_read_account_write_conflict_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.lock_write_account(&pk1, 0);
        locks.lock_read_account(&pk1, 1);
    }

    #[test]
    #[should_panic]
    fn test_unlock_read_account_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.lock_read_account(&pk1, 0);
        locks.unlock_read_account(&pk1, 1);
    }
}
