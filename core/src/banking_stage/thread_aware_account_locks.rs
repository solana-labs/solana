use {
    solana_sdk::pubkey::Pubkey,
    std::{
        collections::HashMap,
        ops::{BitAnd, BitAndAssign},
    },
};

pub const MAX_THREADS: usize = 64;

/// Identifier for a thread
pub type ThreadId = usize; // 0..MAX_THREADS-1

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
    num_threads: usize, // 0..MAX_THREADS
    /// Limit on the number of sequentially-queued transactions per account.
    sequential_queue_limit: u32,
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
    pub fn new(num_threads: usize, write_queue_limit: u32) -> Self {
        assert!(num_threads > 0 && num_threads <= MAX_THREADS);
        Self {
            num_threads,
            sequential_queue_limit: write_queue_limit,
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

        for account in write_account_locks {
            schedulable_threads &= self.write_schedulable_threads(account);
        }

        for account in read_account_locks {
            schedulable_threads &= self.read_schedulable_threads(account);
        }

        schedulable_threads
    }

    /// Returns `ThreadSet` of schedulable threads for the given readable account.
    /// If the account is not locked, then all threads are schedulable.
    /// If only read locked, then all threads are schedulable.
    /// If write-locked, then only the thread holding the write lock is schedulable.
    /// The sequential limit is checked, and a thread will not be returned as schedulable
    /// if the limit is reached.
    fn read_schedulable_threads(&self, account: &Pubkey) -> ThreadSet {
        // If the account is only read locked, then a read lock can be taken on any thread
        // that is not at the sequential limit.
        self.schedulable_threads_with_read_only_handler(account, |thread_set, counts| {
            let mut schedulable_threads = ThreadSet::any(self.num_threads);
            for thread_id in thread_set.threads_iter() {
                if counts[thread_id] == self.sequential_queue_limit {
                    schedulable_threads.remove(thread_id);
                }
            }
            schedulable_threads
        })
    }

    /// Returns `ThreadSet` of schedulable threads for the given writable account.
    /// If the account is not locked, then all threads are schedulable.
    /// If read-locked on a single thread, then only that thread is schedulable.
    /// If write-locked, then only that thread is schedulable.
    /// In all other cases, no threads are schedulable.
    /// The sequential limit is checked, and a thread will not be returned as schedulable
    /// if the limit is reached.
    fn write_schedulable_threads(&self, account: &Pubkey) -> ThreadSet {
        // If the account is only read locked, then a write lock can only be taken
        // if the read lock is held by a single thread, and the limit is not exceeded.
        self.schedulable_threads_with_read_only_handler(account, |thread_set, counts| {
            thread_set
                .only_one_scheduled()
                .filter(|thread_id| counts[*thread_id] < self.sequential_queue_limit)
                .map_or_else(ThreadSet::none, ThreadSet::only)
        })
    }

    /// Returns `ThreadSet` of schedulable threads, given the read-only lock handler.
    /// Helper function, since the only difference between read and write schedulable threads
    /// is in how the case where only read locks are held is handled.
    /// If there are no locks, then all threads are schedulable.
    /// If only write-locked, then only the thread holding the write lock is schedulable.
    /// If a mix of locks, then only the write thread is schedulable.
    /// The sequential limit is checked, and a thread will not be returned as schedulable
    /// if the limit is reached.
    fn schedulable_threads_with_read_only_handler(
        &self,
        account: &Pubkey,
        read_only_handler: impl Fn(&ThreadSet, &[u32]) -> ThreadSet,
    ) -> ThreadSet {
        match (self.write_locks.get(account), self.read_locks.get(account)) {
            (None, None) => ThreadSet::any(self.num_threads),
            (None, Some((thread_set, counts))) => read_only_handler(thread_set, counts),
            (Some((thread_id, count)), None) => {
                if count == &self.sequential_queue_limit {
                    ThreadSet::none()
                } else {
                    ThreadSet::only(*thread_id)
                }
            }
            (Some((thread_id, count)), Some((thread_set, counts))) => {
                debug_assert_eq!(Some(*thread_id), thread_set.only_one_scheduled());
                if count + counts[*thread_id] == self.sequential_queue_limit {
                    ThreadSet::none()
                } else {
                    ThreadSet::only(*thread_id)
                }
            }
        }
    }

    /// Add locks for all writable and readable accounts on `thread_id`.
    pub fn lock_accounts<'a>(
        &mut self,
        write_account_locks: impl Iterator<Item = &'a Pubkey>,
        read_account_locks: impl Iterator<Item = &'a Pubkey>,
        thread_id: ThreadId,
    ) {
        for account in write_account_locks {
            self.write_lock_account(account, thread_id);
        }

        for account in read_account_locks {
            self.read_lock_account(account, thread_id);
        }
    }

    /// Locks the given `account` for writing on `thread_id`.
    /// Panics if the account is already locked for writing on another thread.
    fn write_lock_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.write_locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let (lock_thread_id, lock_count) = entry.get_mut();
                assert_eq!(*lock_thread_id, thread_id);

                *lock_count += 1;
                assert!(*lock_count <= self.sequential_queue_limit);
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
    fn write_unlock_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
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
    fn read_lock_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.read_locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let (thread_set, lock_counts) = entry.get_mut();
                assert!(thread_set.contains(thread_id));

                lock_counts[thread_id] += 1;
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let mut lock_counts = [0; MAX_THREADS];
                lock_counts[thread_id] = 1;
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
    fn read_unlock_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        match self.read_locks.entry(*account) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let (thread_set, lock_counts) = entry.get_mut();
                assert!(thread_set.contains(thread_id));
                lock_counts[thread_id] -= 1;
                if lock_counts[thread_id] == 0 {
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
    pub fn any(num_threads: usize) -> Self {
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

    const TEST_NUM_THREADS: usize = 4;

    #[test]
    #[should_panic]
    fn test_too_few_num_threads() {
        ThreadAwareAccountLocks::new(0, 1);
    }

    #[test]
    #[should_panic]
    fn test_too_many_num_threads() {
        ThreadAwareAccountLocks::new(MAX_THREADS + 1, 1);
    }

    #[test]
    fn test_accounts_schedulable_threads() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);

        // No locks - all threads are schedulable
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::any(TEST_NUM_THREADS)
        );

        // Write lock on pk1 on thread 0 - now only thread 0 is schedulable
        locks.write_lock_account(&pk1, 0);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::only(0)
        );

        // Write lock pk2 on thread 0 - can still schedule on thread 0
        locks.write_lock_account(&pk2, 0);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::only(0)
        );

        // Move pk2 lock to thread 1 - cannot schedule on any threads
        locks.write_unlock_account(&pk2, 0);
        locks.write_lock_account(&pk2, 1);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::none()
        );

        // Remove pk2 lock, add another lock for pk1 on thread 0 - at `write_queue_limit` so cannot schedule on any threads
        locks.write_unlock_account(&pk2, 1);
        locks.write_lock_account(&pk1, 0);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            ThreadSet::none()
        );
    }

    #[test]
    fn test_locks() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.write_lock_account(&pk1, 0);
        locks.write_lock_account(&pk1, 0);

        locks.read_lock_account(&pk2, 1);
        locks.write_lock_account(&pk2, 1);
    }

    #[test]
    #[should_panic]
    fn test_write_lock_account_write_conflict_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.write_lock_account(&pk1, 0);
        locks.write_lock_account(&pk1, 1);
    }

    #[test]
    #[should_panic]
    fn test_write_lock_account_read_conflict_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.read_lock_account(&pk1, 0);
        locks.write_lock_account(&pk1, 1);
    }

    #[test]
    #[should_panic]
    fn test_write_unlock_account_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.write_lock_account(&pk1, 1);
        locks.write_unlock_account(&pk1, 0);
    }

    #[test]
    #[should_panic]
    fn test_read_lock_account_write_conflict_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.write_lock_account(&pk1, 0);
        locks.read_lock_account(&pk1, 1);
    }

    #[test]
    #[should_panic]
    fn test_read_unlock_account_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
        locks.read_lock_account(&pk1, 0);
        locks.read_unlock_account(&pk1, 1);
    }

    #[test]
    fn test_thread_set() {
        let mut thread_set = ThreadSet::none();
        assert!(thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 0);
        assert_eq!(thread_set.only_one_scheduled(), None);
        for idx in 0..MAX_THREADS {
            assert!(!thread_set.contains(idx));
        }

        thread_set.insert(4);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 1);
        assert_eq!(thread_set.only_one_scheduled(), Some(4));
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx), idx == 4);
        }

        thread_set.insert(2);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 2);
        assert_eq!(thread_set.only_one_scheduled(), None);
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx), idx == 2 || idx == 4);
        }

        thread_set.remove(4);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 1);
        assert_eq!(thread_set.only_one_scheduled(), Some(2));
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx), idx == 2);
        }
    }
}
