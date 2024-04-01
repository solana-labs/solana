use {
    solana_sdk::pubkey::Pubkey,
    std::{
        collections::{hash_map::Entry, HashMap},
        fmt::{Debug, Display},
        ops::{BitAnd, BitAndAssign, Sub},
    },
};

pub(crate) const MAX_THREADS: usize = u64::BITS as usize;

/// Identifier for a thread
pub(crate) type ThreadId = usize; // 0..MAX_THREADS-1

type LockCount = u32;

/// A bit-set of threads an account is scheduled or can be scheduled for.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct ThreadSet(u64);

struct AccountWriteLocks {
    thread_id: ThreadId,
    lock_count: LockCount,
}

struct AccountReadLocks {
    thread_set: ThreadSet,
    lock_counts: [LockCount; MAX_THREADS],
}

/// Account locks.
/// Write Locks - only one thread can hold a write lock at a time.
///     Contains how many write locks are held by the thread.
/// Read Locks - multiple threads can hold a read lock at a time.
///     Contains thread-set for easily checking which threads are scheduled.
#[derive(Default)]
struct AccountLocks {
    pub write_locks: Option<AccountWriteLocks>,
    pub read_locks: Option<AccountReadLocks>,
}

/// Thread-aware account locks which allows for scheduling on threads
/// that already hold locks on the account. This is useful for allowing
/// queued transactions to be scheduled on a thread while the transaction
/// is still being executed on the thread.
pub(crate) struct ThreadAwareAccountLocks {
    /// Number of threads.
    num_threads: usize, // 0..MAX_THREADS
    /// Locks for each account. An account should only have an entry if there
    /// is at least one lock.
    locks: HashMap<Pubkey, AccountLocks>,
}

impl ThreadAwareAccountLocks {
    /// Creates a new `ThreadAwareAccountLocks` with the given number of threads.
    pub(crate) fn new(num_threads: usize) -> Self {
        assert!(num_threads > 0, "num threads must be > 0");
        assert!(
            num_threads <= MAX_THREADS,
            "num threads must be <= {MAX_THREADS}"
        );

        Self {
            num_threads,
            locks: HashMap::new(),
        }
    }

    /// Returns the `ThreadId` if the accounts are able to be locked
    /// for the given thread, otherwise `None` is returned.
    /// `allowed_threads` is a set of threads that the caller restricts locking to.
    /// If accounts are schedulable, then they are locked for the thread
    /// selected by the `thread_selector` function.
    /// `thread_selector` is only called if all accounts are schdulable, meaning
    /// that the `thread_set` passed to `thread_selector` is non-empty.
    pub(crate) fn try_lock_accounts<'a>(
        &mut self,
        write_account_locks: impl Iterator<Item = &'a Pubkey> + Clone,
        read_account_locks: impl Iterator<Item = &'a Pubkey> + Clone,
        allowed_threads: ThreadSet,
        thread_selector: impl FnOnce(ThreadSet) -> ThreadId,
    ) -> Option<ThreadId> {
        let schedulable_threads = self.accounts_schedulable_threads(
            write_account_locks.clone(),
            read_account_locks.clone(),
        )? & allowed_threads;
        (!schedulable_threads.is_empty()).then(|| {
            let thread_id = thread_selector(schedulable_threads);
            self.lock_accounts(write_account_locks, read_account_locks, thread_id);
            thread_id
        })
    }

    /// Unlocks the accounts for the given thread.
    pub(crate) fn unlock_accounts<'a>(
        &mut self,
        write_account_locks: impl Iterator<Item = &'a Pubkey>,
        read_account_locks: impl Iterator<Item = &'a Pubkey>,
        thread_id: ThreadId,
    ) {
        for account in write_account_locks {
            self.write_unlock_account(account, thread_id);
        }

        for account in read_account_locks {
            self.read_unlock_account(account, thread_id);
        }
    }

    /// Returns `ThreadSet` that the given accounts can be scheduled on.
    fn accounts_schedulable_threads<'a>(
        &self,
        write_account_locks: impl Iterator<Item = &'a Pubkey>,
        read_account_locks: impl Iterator<Item = &'a Pubkey>,
    ) -> Option<ThreadSet> {
        let mut schedulable_threads = ThreadSet::any(self.num_threads);

        for account in write_account_locks {
            schedulable_threads &= self.write_schedulable_threads(account);
            if schedulable_threads.is_empty() {
                return None;
            }
        }

        for account in read_account_locks {
            schedulable_threads &= self.read_schedulable_threads(account);
            if schedulable_threads.is_empty() {
                return None;
            }
        }

        Some(schedulable_threads)
    }

    /// Returns `ThreadSet` of schedulable threads for the given readable account.
    fn read_schedulable_threads(&self, account: &Pubkey) -> ThreadSet {
        self.schedulable_threads::<false>(account)
    }

    /// Returns `ThreadSet` of schedulable threads for the given writable account.
    fn write_schedulable_threads(&self, account: &Pubkey) -> ThreadSet {
        self.schedulable_threads::<true>(account)
    }

    /// Returns `ThreadSet` of schedulable threads.
    /// If there are no locks, then all threads are schedulable.
    /// If only write-locked, then only the thread holding the write lock is schedulable.
    /// If a mix of locks, then only the write thread is schedulable.
    /// If only read-locked, the only write-schedulable thread is if a single thread
    ///   holds all read locks. Otherwise, no threads are write-schedulable.
    /// If only read-locked, all threads are read-schedulable.
    fn schedulable_threads<const WRITE: bool>(&self, account: &Pubkey) -> ThreadSet {
        match self.locks.get(account) {
            None => ThreadSet::any(self.num_threads),
            Some(AccountLocks {
                write_locks: None,
                read_locks: Some(read_locks),
            }) => {
                if WRITE {
                    read_locks
                        .thread_set
                        .only_one_contained()
                        .map(ThreadSet::only)
                        .unwrap_or_else(ThreadSet::none)
                } else {
                    ThreadSet::any(self.num_threads)
                }
            }
            Some(AccountLocks {
                write_locks: Some(write_locks),
                read_locks: None,
            }) => ThreadSet::only(write_locks.thread_id),
            Some(AccountLocks {
                write_locks: Some(write_locks),
                read_locks: Some(read_locks),
            }) => {
                assert_eq!(
                    read_locks.thread_set.only_one_contained(),
                    Some(write_locks.thread_id)
                );
                read_locks.thread_set
            }
            Some(AccountLocks {
                write_locks: None,
                read_locks: None,
            }) => unreachable!(),
        }
    }

    /// Add locks for all writable and readable accounts on `thread_id`.
    fn lock_accounts<'a>(
        &mut self,
        write_account_locks: impl Iterator<Item = &'a Pubkey>,
        read_account_locks: impl Iterator<Item = &'a Pubkey>,
        thread_id: ThreadId,
    ) {
        assert!(
            thread_id < self.num_threads,
            "thread_id must be < num_threads"
        );
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
        let entry = self.locks.entry(*account).or_default();

        let AccountLocks {
            write_locks,
            read_locks,
        } = entry;

        if let Some(read_locks) = read_locks {
            assert_eq!(
                read_locks.thread_set.only_one_contained(),
                Some(thread_id),
                "outstanding read lock must be on same thread"
            );
        }

        if let Some(write_locks) = write_locks {
            assert_eq!(
                write_locks.thread_id, thread_id,
                "outstanding write lock must be on same thread"
            );
            write_locks.lock_count += 1;
        } else {
            *write_locks = Some(AccountWriteLocks {
                thread_id,
                lock_count: 1,
            });
        }
    }

    /// Unlocks the given `account` for writing on `thread_id`.
    /// Panics if the account is not locked for writing on `thread_id`.
    fn write_unlock_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        let Entry::Occupied(mut entry) = self.locks.entry(*account) else {
            panic!("write lock must exist for account: {account}");
        };

        let AccountLocks {
            write_locks: maybe_write_locks,
            read_locks,
        } = entry.get_mut();

        let Some(write_locks) = maybe_write_locks else {
            panic!("write lock must exist for account: {account}");
        };

        assert_eq!(
            write_locks.thread_id, thread_id,
            "outstanding write lock must be on same thread"
        );

        write_locks.lock_count -= 1;
        if write_locks.lock_count == 0 {
            *maybe_write_locks = None;
            if read_locks.is_none() {
                entry.remove();
            }
        }
    }

    /// Locks the given `account` for reading on `thread_id`.
    /// Panics if the account is already locked for writing on another thread.
    fn read_lock_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        let AccountLocks {
            write_locks,
            read_locks,
        } = self.locks.entry(*account).or_default();

        if let Some(write_locks) = write_locks {
            assert_eq!(
                write_locks.thread_id, thread_id,
                "outstanding write lock must be on same thread"
            );
        }

        match read_locks {
            Some(read_locks) => {
                read_locks.thread_set.insert(thread_id);
                read_locks.lock_counts[thread_id] += 1;
            }
            None => {
                let mut lock_counts = [0; MAX_THREADS];
                lock_counts[thread_id] = 1;
                *read_locks = Some(AccountReadLocks {
                    thread_set: ThreadSet::only(thread_id),
                    lock_counts,
                });
            }
        }
    }

    /// Unlocks the given `account` for reading on `thread_id`.
    /// Panics if the account is not locked for reading on `thread_id`.
    fn read_unlock_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
        let Entry::Occupied(mut entry) = self.locks.entry(*account) else {
            panic!("read lock must exist for account: {account}");
        };

        let AccountLocks {
            write_locks,
            read_locks: maybe_read_locks,
        } = entry.get_mut();

        let Some(read_locks) = maybe_read_locks else {
            panic!("read lock must exist for account: {account}");
        };

        assert!(
            read_locks.thread_set.contains(thread_id),
            "outstanding read lock must be on same thread"
        );

        read_locks.lock_counts[thread_id] -= 1;
        if read_locks.lock_counts[thread_id] == 0 {
            read_locks.thread_set.remove(thread_id);
            if read_locks.thread_set.is_empty() {
                *maybe_read_locks = None;
                if write_locks.is_none() {
                    entry.remove();
                }
            }
        }
    }
}

impl BitAnd for ThreadSet {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for ThreadSet {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl Sub for ThreadSet {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 & !rhs.0)
    }
}

impl Display for ThreadSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ThreadSet({:#0width$b})", self.0, width = MAX_THREADS)
    }
}

impl Debug for ThreadSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}

impl ThreadSet {
    #[inline(always)]
    pub(crate) const fn none() -> Self {
        Self(0b0)
    }

    #[inline(always)]
    pub(crate) const fn any(num_threads: usize) -> Self {
        if num_threads == MAX_THREADS {
            Self(u64::MAX)
        } else {
            Self(Self::as_flag(num_threads) - 1)
        }
    }

    #[inline(always)]
    pub(crate) const fn only(thread_id: ThreadId) -> Self {
        Self(Self::as_flag(thread_id))
    }

    #[inline(always)]
    pub(crate) fn num_threads(&self) -> u32 {
        self.0.count_ones()
    }

    #[inline(always)]
    pub(crate) fn only_one_contained(&self) -> Option<ThreadId> {
        (self.num_threads() == 1).then_some(self.0.trailing_zeros() as ThreadId)
    }

    #[inline(always)]
    pub(crate) fn is_empty(&self) -> bool {
        self == &Self::none()
    }

    #[inline(always)]
    pub(crate) fn contains(&self, thread_id: ThreadId) -> bool {
        self.0 & Self::as_flag(thread_id) != 0
    }

    #[inline(always)]
    pub(crate) fn insert(&mut self, thread_id: ThreadId) {
        self.0 |= Self::as_flag(thread_id);
    }

    #[inline(always)]
    pub(crate) fn remove(&mut self, thread_id: ThreadId) {
        self.0 &= !Self::as_flag(thread_id);
    }

    #[inline(always)]
    pub(crate) fn contained_threads_iter(self) -> impl Iterator<Item = ThreadId> {
        (0..MAX_THREADS).filter(move |thread_id| self.contains(*thread_id))
    }

    #[inline(always)]
    const fn as_flag(thread_id: ThreadId) -> u64 {
        0b1 << thread_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_NUM_THREADS: usize = 4;
    const TEST_ANY_THREADS: ThreadSet = ThreadSet::any(TEST_NUM_THREADS);

    // Simple thread selector to select the first schedulable thread
    fn test_thread_selector(thread_set: ThreadSet) -> ThreadId {
        thread_set.contained_threads_iter().next().unwrap()
    }

    #[test]
    #[should_panic(expected = "num threads must be > 0")]
    fn test_too_few_num_threads() {
        ThreadAwareAccountLocks::new(0);
    }

    #[test]
    #[should_panic(expected = "num threads must be <=")]
    fn test_too_many_num_threads() {
        ThreadAwareAccountLocks::new(MAX_THREADS + 1);
    }

    #[test]
    fn test_try_lock_accounts_none() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.read_lock_account(&pk1, 2);
        locks.read_lock_account(&pk1, 3);
        assert_eq!(
            locks.try_lock_accounts(
                [&pk1].into_iter(),
                [&pk2].into_iter(),
                TEST_ANY_THREADS,
                test_thread_selector
            ),
            None
        );
    }

    #[test]
    fn test_try_lock_accounts_one() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.write_lock_account(&pk2, 3);

        assert_eq!(
            locks.try_lock_accounts(
                [&pk1].into_iter(),
                [&pk2].into_iter(),
                TEST_ANY_THREADS,
                test_thread_selector
            ),
            Some(3)
        );
    }

    #[test]
    fn test_try_lock_accounts_multiple() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.read_lock_account(&pk2, 0);
        locks.read_lock_account(&pk2, 0);

        assert_eq!(
            locks.try_lock_accounts(
                [&pk1].into_iter(),
                [&pk2].into_iter(),
                TEST_ANY_THREADS - ThreadSet::only(0), // exclude 0
                test_thread_selector
            ),
            Some(1)
        );
    }

    #[test]
    fn test_try_lock_accounts_any() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        assert_eq!(
            locks.try_lock_accounts(
                [&pk1].into_iter(),
                [&pk2].into_iter(),
                TEST_ANY_THREADS,
                test_thread_selector
            ),
            Some(0)
        );
    }

    #[test]
    fn test_accounts_schedulable_threads_no_outstanding_locks() {
        let pk1 = Pubkey::new_unique();
        let locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);

        assert_eq!(
            locks.accounts_schedulable_threads([&pk1].into_iter(), std::iter::empty()),
            Some(TEST_ANY_THREADS)
        );
        assert_eq!(
            locks.accounts_schedulable_threads(std::iter::empty(), [&pk1].into_iter()),
            Some(TEST_ANY_THREADS)
        );
    }

    #[test]
    fn test_accounts_schedulable_threads_outstanding_write_only() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);

        locks.write_lock_account(&pk1, 2);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            Some(ThreadSet::only(2))
        );
        assert_eq!(
            locks.accounts_schedulable_threads(std::iter::empty(), [&pk1, &pk2].into_iter()),
            Some(ThreadSet::only(2))
        );
    }

    #[test]
    fn test_accounts_schedulable_threads_outstanding_read_only() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);

        locks.read_lock_account(&pk1, 2);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            Some(ThreadSet::only(2))
        );
        assert_eq!(
            locks.accounts_schedulable_threads(std::iter::empty(), [&pk1, &pk2].into_iter()),
            Some(TEST_ANY_THREADS)
        );

        locks.read_lock_account(&pk1, 0);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            None
        );
        assert_eq!(
            locks.accounts_schedulable_threads(std::iter::empty(), [&pk1, &pk2].into_iter()),
            Some(TEST_ANY_THREADS)
        );
    }

    #[test]
    fn test_accounts_schedulable_threads_outstanding_mixed() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);

        locks.read_lock_account(&pk1, 2);
        locks.write_lock_account(&pk1, 2);
        assert_eq!(
            locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
            Some(ThreadSet::only(2))
        );
        assert_eq!(
            locks.accounts_schedulable_threads(std::iter::empty(), [&pk1, &pk2].into_iter()),
            Some(ThreadSet::only(2))
        );
    }

    #[test]
    #[should_panic(expected = "outstanding write lock must be on same thread")]
    fn test_write_lock_account_write_conflict_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.write_lock_account(&pk1, 0);
        locks.write_lock_account(&pk1, 1);
    }

    #[test]
    #[should_panic(expected = "outstanding read lock must be on same thread")]
    fn test_write_lock_account_read_conflict_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.read_lock_account(&pk1, 0);
        locks.write_lock_account(&pk1, 1);
    }

    #[test]
    #[should_panic(expected = "write lock must exist")]
    fn test_write_unlock_account_not_locked() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.write_unlock_account(&pk1, 0);
    }

    #[test]
    #[should_panic(expected = "outstanding write lock must be on same thread")]
    fn test_write_unlock_account_thread_mismatch() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.write_lock_account(&pk1, 1);
        locks.write_unlock_account(&pk1, 0);
    }

    #[test]
    #[should_panic(expected = "outstanding write lock must be on same thread")]
    fn test_read_lock_account_write_conflict_panic() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.write_lock_account(&pk1, 0);
        locks.read_lock_account(&pk1, 1);
    }

    #[test]
    #[should_panic(expected = "read lock must exist")]
    fn test_read_unlock_account_not_locked() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.read_unlock_account(&pk1, 1);
    }

    #[test]
    #[should_panic(expected = "outstanding read lock must be on same thread")]
    fn test_read_unlock_account_thread_mismatch() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.read_lock_account(&pk1, 0);
        locks.read_unlock_account(&pk1, 1);
    }

    #[test]
    fn test_write_locking() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.write_lock_account(&pk1, 1);
        locks.write_lock_account(&pk1, 1);
        locks.write_unlock_account(&pk1, 1);
        locks.write_unlock_account(&pk1, 1);
        assert!(locks.locks.is_empty());
    }

    #[test]
    fn test_read_locking() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.read_lock_account(&pk1, 1);
        locks.read_lock_account(&pk1, 1);
        locks.read_unlock_account(&pk1, 1);
        locks.read_unlock_account(&pk1, 1);
        assert!(locks.locks.is_empty());
    }

    #[test]
    #[should_panic(expected = "thread_id must be < num_threads")]
    fn test_lock_accounts_invalid_thread() {
        let pk1 = Pubkey::new_unique();
        let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS);
        locks.lock_accounts([&pk1].into_iter(), std::iter::empty(), TEST_NUM_THREADS);
    }

    #[test]
    fn test_thread_set() {
        let mut thread_set = ThreadSet::none();
        assert!(thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 0);
        assert_eq!(thread_set.only_one_contained(), None);
        for idx in 0..MAX_THREADS {
            assert!(!thread_set.contains(idx));
        }

        thread_set.insert(4);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 1);
        assert_eq!(thread_set.only_one_contained(), Some(4));
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx), idx == 4);
        }

        thread_set.insert(2);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 2);
        assert_eq!(thread_set.only_one_contained(), None);
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx), idx == 2 || idx == 4);
        }

        thread_set.remove(4);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 1);
        assert_eq!(thread_set.only_one_contained(), Some(2));
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx), idx == 2);
        }
    }

    #[test]
    fn test_thread_set_any_zero() {
        let any_threads = ThreadSet::any(0);
        assert_eq!(any_threads.num_threads(), 0);
    }

    #[test]
    fn test_thread_set_any_max() {
        let any_threads = ThreadSet::any(MAX_THREADS);
        assert_eq!(any_threads.num_threads(), MAX_THREADS as u32);
    }
}
