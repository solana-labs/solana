use {
    solana_sdk::pubkey::Pubkey,
    std::{
        collections::HashMap,
        ops::{BitAnd, BitAndAssign},
    },
};

pub const MAX_THREADS: usize = 64;

/// Identifier for a thread
pub type ThreadId = u8; // 0..MAX_THREADS-1

/// A bit-set of threads an account is scheduled or can be scheduled for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct ThreadSet(u64);

impl BitAndAssign for ThreadSet {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl BitAnd for ThreadSet {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

/// Thread-aware account locks which allows for scheduling on threads
/// that already hold locks on the account. This is useful for allowing
/// queued transactions to be scheduled on a thread while the transaction
/// is still being executed on the thread, up to a queue limit.
pub struct ThreadAwareAccountLocks {
    /// Number of threads.
    num_threads: u8, // 0..MAX_THREADS
    /// Limit on the number of write-queued transactions per account.
    write_queue_limit: u32,
    /// Write locks - only on thread can hold a write lock at a time.
    /// Contains how many write locks are held by the thread.
    write_locks: HashMap<Pubkey, (ThreadId, u32)>,
    /// Read locks - multiple threads can hold a read lock at a time.
    /// Contains thread-set for easily checking which threads are scheduled.
    /// Contains how many read locks are held by each thread.
    read_locks: HashMap<Pubkey, (ThreadSet, [u32; MAX_THREADS])>,
}

impl ThreadAwareAccountLocks {
    /// Creates a new `ThreadAwareAccountLocks` with the given number of threads
    /// and queue limit.
    pub fn new(num_threads: u8, write_queue_limit: u32) -> Self {
        assert!(num_threads <= MAX_THREADS as u8);
        Self {
            num_threads,
            write_queue_limit,
            write_locks: HashMap::new(),
            read_locks: HashMap::new(),
        }
    }

    /// Returns `ThreadSet` that the given accounts can be scheduled on.
    pub fn accounts_schedulable_threads<'a>(
        &self,
        write_account_locks: impl Iterator<Item = &'a Pubkey>,
        read_account_locks: impl Iterator<Item = &'a Pubkey>,
    ) -> ThreadSet {
        let mut schedulable_threads = ThreadSet::any(self.num_threads);

        // Write accounts conflict with read and write locks.
        for account in write_account_locks {
            if let Some((thread_id, count)) = self.write_locks.get(account) {
                if count == &self.write_queue_limit {
                    return ThreadSet::none();
                }
                schedulable_threads &= ThreadSet::only(*thread_id);
            }
            if let Some((thread_set, _)) = self.read_locks.get(account) {
                schedulable_threads &= *thread_set;
            }
        }

        // Read accounts conflict with write locks.
        for account in read_account_locks {
            if let Some((thread_id, _)) = self.write_locks.get(account) {
                schedulable_threads &= ThreadSet::only(*thread_id);
            }
        }

        schedulable_threads
    }

    /// Add locks for all writable and readable accounts on `thread_id`.
    pub fn lock_accounts<'a>(
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
        match self.write_locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let (lock_thread_id, lock_count) = entry.get_mut();
                assert_eq!(*lock_thread_id, thread_id);

                *lock_count += 1;
                assert!(*lock_count <= self.write_queue_limit);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((thread_id, 1));
            }
        }

        // Check for outstanding read-locks
        if let Some((read_thread_set, _)) = self.read_locks.get(account) {
            assert_eq!(read_thread_set, &ThreadSet::only(thread_id));
        }
    }

    /// Unlocks the given `account` for writing on `thread_id`.
    /// Panics if the account is not locked for writing on `thread_id`.
    fn unlock_write_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.write_locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let (lock_thread_id, lock_count) = entry.get_mut();
                assert_eq!(*lock_thread_id, thread_id);
                *lock_count -= 1;
                if *lock_count == 0 {
                    entry.remove();
                }
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                panic!("Write lock not found for account: {account}");
            }
        }
    }

    /// Locks the given `account` for reading on `thread_id`.
    /// Panics if the account is already locked for writing on another thread.
    fn lock_read_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.read_locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let (thread_set, lock_counts) = entry.get_mut();
                assert!(thread_set.contains(thread_id));

                lock_counts[thread_id as usize] += 1;
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let mut lock_counts = [0; MAX_THREADS];
                lock_counts[thread_id as usize] = 1;
                entry.insert((ThreadSet::only(thread_id), lock_counts));
            }
        }

        // Check for outstanding write-locks
        if let Some((write_thread_id, _)) = self.write_locks.get(account) {
            assert_eq!(write_thread_id, &thread_id);
        }
    }

    /// Unlocks the given `account` for reading on `thread_id`.
    /// Panics if the account is not locked for reading on `thread_id`.
    fn unlock_read_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.read_locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let (thread_set, lock_counts) = entry.get_mut();
                assert!(thread_set.contains(thread_id));
                lock_counts[thread_id as usize] -= 1;
                if lock_counts[thread_id as usize] == 0 {
                    thread_set.remove(thread_id);
                    if thread_set.is_empty() {
                        entry.remove();
                    }
                }
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                panic!("Read lock not found for account: {account}");
            }
        }
    }
}

impl ThreadSet {
    #[inline(always)]
    pub fn none() -> Self {
        Self(0)
    }

    #[inline(always)]
    pub fn any(num_threads: u8) -> Self {
        Self((1 << num_threads) - 1)
    }

    #[inline(always)]
    pub fn only(thread_id: ThreadId) -> Self {
        Self(1 << thread_id)
    }

    #[inline(always)]
    pub fn num_threads(&self) -> u8 {
        self.0.count_ones() as u8
    }

    #[inline(always)]
    pub fn only_one_scheduled(&self) -> Option<ThreadId> {
        (self.num_threads() == 1).then_some(self.0.trailing_zeros() as ThreadId)
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    #[inline(always)]
    pub fn contains(&self, thread_id: ThreadId) -> bool {
        self.0 & (1 << thread_id) != 0
    }

    #[inline(always)]
    pub fn insert(&mut self, thread_id: ThreadId) {
        self.0 |= 1 << thread_id;
    }

    #[inline(always)]
    pub fn remove(&mut self, thread_id: ThreadId) {
        self.0 &= !(1 << thread_id);
    }

    #[inline(always)]
    pub fn threads_iter(self) -> impl Iterator<Item = ThreadId> {
        (0..MAX_THREADS as ThreadId).filter(move |thread_id| self.contains(*thread_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_NUM_THREADS: u8 = 4;

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

    #[test]
    fn test_thread_set() {
        let mut thread_set = ThreadSet::none();
        assert!(thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 0);
        assert_eq!(thread_set.only_one_scheduled(), None);
        for idx in 0..MAX_THREADS {
            assert!(!thread_set.contains(idx as ThreadId));
        }

        thread_set.insert(4);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 1);
        assert_eq!(thread_set.only_one_scheduled(), Some(4));
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx as ThreadId), idx == 4);
        }

        thread_set.insert(2);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 2);
        assert_eq!(thread_set.only_one_scheduled(), None);
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx as ThreadId), idx == 2 || idx == 4);
        }

        thread_set.remove(4);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 1);
        assert_eq!(thread_set.only_one_scheduled(), Some(2));
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx as ThreadId), idx == 2);
        }
    }
}
