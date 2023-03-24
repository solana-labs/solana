// use {
//     solana_sdk::pubkey::Pubkey,
//     std::{
//         collections::{HashMap, VecDeque},
//         ops::{BitAnd, BitAndAssign},
//     },
// };

// pub const MAX_THREADS: usize = 64;

// /// Identifier for a thread
// pub type ThreadId = u8; // 0..MAX_THREADS-1

// /// A bit-set of threads an account is scheduled or can be scheduled for.
// #[derive(Copy, Clone, Debug, PartialEq, Eq)]
// #[repr(transparent)]
// pub struct ThreadSet(u64);

// /// Thread-aware account locks which allows for scheduling on threads
// /// that already hold locks on the account. This is useful for allowing
// /// queued transactions to be scheduled on a thread while the transaction
// /// is still being executed on the thread, up to a queue limit.
// pub struct ThreadAwareAccountLocks {
//     /// Number of threads.
//     num_threads: u8, // 0..MAX_THREADS
//     /// Limit on the number of sequentially queued transactions per account.
//     sequential_queue_limit: u32,
//     /// Locks held for each account.
//     locks: HashMap<Pubkey, AccountLock>,
// }

// impl ThreadAwareAccountLocks {
//     /// Creates a new `ThreadAwareAccountLocks` with the given number of threads
//     /// and queue limit.
//     pub fn new(num_threads: u8, sequential_queue_limit: u32) -> Self {
//         assert!(num_threads > 0 && num_threads <= MAX_THREADS as u8);
//         Self {
//             num_threads,
//             sequential_queue_limit,
//             locks: HashMap::new(),
//         }
//     }

//     /// Returns `ThreadSet` that the given accounts can be scheduled on.
//     pub fn accounts_schedulable_threads<'a>(
//         &self,
//         write_account_locks: impl Iterator<Item = &'a Pubkey>,
//         read_account_locks: impl Iterator<Item = &'a Pubkey>,
//     ) -> ThreadSet {
//         let mut schedulable_threads = ThreadSet::any(self.num_threads);

//         for account_lock in write_account_locks.filter_map(|account| self.locks.get(account)) {
//             schedulable_threads &= account_lock.write_schedulable(self.sequential_queue_limit);
//         }

//         for account_lock in read_account_locks.filter_map(|account| self.locks.get(account)) {
//             schedulable_threads &=
//                 account_lock.read_schedulable(self.num_threads, self.sequential_queue_limit);
//         }

//         schedulable_threads
//     }

//     /// Add locks for all writable and readable accounts on `thread_id`.
//     pub fn lock_accounts<'a>(
//         &mut self,
//         write_account_locks: impl Iterator<Item = &'a Pubkey>,
//         read_account_locks: impl Iterator<Item = &'a Pubkey>,
//         thread_id: ThreadId,
//     ) {
//         for account in write_account_locks {
//             self.lock_write_account(account, thread_id);
//         }

//         for account in read_account_locks {
//             self.lock_read_account(account, thread_id);
//         }
//     }

//     /// Locks the given `account` for writing on `thread_id`.
//     /// Panics if the account is already locked for writing on another thread.
//     fn lock_write_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
//         match self.locks.entry(*account) {
//             std::collections::hash_map::Entry::Occupied(entry) => {
//                 entry.into_mut().lock_write(thread_id);
//             }
//             std::collections::hash_map::Entry::Vacant(entry) => {
//                 entry.insert(AccountLock::new_write_lock(thread_id, 1));
//             }
//         }
//     }

//     /// Unlocks the given `account` for writing on `thread_id`.
//     /// Panics if the account is not locked for writing on `thread_id`.
//     fn unlock_write_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
//         match self.locks.entry(*account) {
//             std::collections::hash_map::Entry::Occupied(entry) => {
//                 entry.into_mut().unlock_write(thread_id);
//             }
//             std::collections::hash_map::Entry::Vacant(_) => {
//                 panic!("No locks not found for account: {account}");
//             }
//         }
//     }

//     /// Locks the given `account` for reading on `thread_id`.
//     /// Panics if the account is already locked for writing on another thread.
//     fn lock_read_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
//         match self.locks.entry(*account) {
//             std::collections::hash_map::Entry::Occupied(entry) => {
//                 entry.into_mut().lock_read(thread_id);
//             }
//             std::collections::hash_map::Entry::Vacant(entry) => {
//                 entry.insert(AccountLock::new_read_lock(thread_id, 1));
//             }
//         }
//     }

//     /// Unlocks the given `account` for reading on `thread_id`.
//     /// Panics if the account is not locked for reading on `thread_id`.
//     fn unlock_read_account(&mut self, account: &Pubkey, thread_id: ThreadId) {
//         match self.locks.entry(*account) {
//             std::collections::hash_map::Entry::Occupied(entry) => {
//                 entry.into_mut().unlock_read(thread_id);
//             }
//             std::collections::hash_map::Entry::Vacant(_) => {
//                 panic!("No locks not found for account: {account}");
//             }
//         }
//     }
// }

// #[derive(Debug)]
// enum AccountLock {
//     ReadOnly(Box<ReadOnlyLock>),
//     WriteOnly(WriteOnlyLock),
//     Mixed(MixedLocks),
// }

// impl AccountLock {
//     /// Creates a new `ReadOnly` lock on `thread_id` with `count` read locks.
//     fn new_read_lock(thread_id: ThreadId, count: u32) -> Self {
//         let mut read_locks = [0; MAX_THREADS];
//         read_locks[thread_id as usize] = count;
//         Self::ReadOnly(Box::new(ReadOnlyLock {
//             thread_set: ThreadSet::only(thread_id),
//             read_locks,
//         }))
//     }

//     /// Creates a new `WriteOnly` lock on `thread_id` with `count` write locks.
//     fn new_write_lock(thread_id: ThreadId, count: u32) -> Self {
//         Self::WriteOnly(WriteOnlyLock { thread_id, count })
//     }

//     /// Returns set of threads that can be read-scheduled.
//     /// If the lock is:
//     ///     * `ReadOnly` - the `Any` thread-set is returned.
//     ///     * `WriteOnly` - thread-set including `thread_id` is returned.
//     ///     * `Mixed` - thread-set including `thread_id` is returned.
//     fn read_schedulable(&self, num_threads: u8, sequential_queue_limit: u32) -> ThreadSet {
//         match self {
//             Self::ReadOnly(locks) => locks.read_schedulable(num_threads, sequential_queue_limit),
//             Self::WriteOnly(locks) => locks.schedulable(sequential_queue_limit),
//             Self::Mixed(locks) => locks.schedulable(sequential_queue_limit),
//         }
//     }

//     /// Returns set of threads that can be write-scheduled.
//     /// If the lock is:
//     ///     * `ReadOnly` - the `Any` thread-set is returned.
//     ///     * `WriteOnly` - thread-set including `thread_id` is returned.
//     ///     * `Mixed` - thread-set including `thread_id` is returned.
//     fn write_schedulable(&self, sequential_queue_limit: u32) -> ThreadSet {
//         match self {
//             Self::ReadOnly(locks) => locks.write_schedulable(sequential_queue_limit),
//             Self::WriteOnly(locks) => locks.schedulable(sequential_queue_limit),
//             Self::Mixed(locks) => locks.schedulable(sequential_queue_limit),
//         }
//     }

//     /// Adds a read lock on `thread_id`, and panics if that is not possible due to write-conflict.
//     /// If the lock is:
//     ///     * `ReadOnly` -  the thread-set is updated and the read-lock count is incremented.
//     ///     * `WriteOnly` - the lock is converted to `Mixed` and the read-lock is added.
//     ///     * `Mixed` - the read-lock is added to the back of the lock-sequence.
//     fn lock_read(&mut self, thread_id: ThreadId) {
//         match self {
//             Self::ReadOnly(locks) => locks.read_lock(thread_id),
//             Self::WriteOnly(locks) => *self = locks.read_lock(thread_id),
//             Self::Mixed(locks) => locks.read_lock(thread_id),
//         }
//     }

//     /// Removes a read lock on `thread_id`, and panics if the lock is not held by the thread.
//     /// If the lock is:
//     ///     * `ReadOnly` -  the thread-set is updated and the read-lock count is decremented.
//     ///     * `Mixed` - the read-lock is removed from the front of the lock-sequence. If all
//     ///                 read-locks are removed, the lock is converted to `WriteOnly`. Mixed
//     ///                 locks can never result in returning true because there are at least
//     ///                 two locks in the sequence.
//     fn unlock_read(&mut self, thread_id: ThreadId) -> bool {
//         match self {
//             Self::ReadOnly(locks) => locks.read_unlock(thread_id),
//             Self::Mixed(locks) => {
//                 if locks.read_unlock(thread_id) {
//                     *self = locks.to_write_only();
//                 }
//                 false
//             }
//             Self::WriteOnly(_) => panic!("cannot unlock_read write locks"),
//         }
//     }

//     /// Adds a write lock on `thread_id`, and panics if that is not possible due to conflicts.
//     /// If the lock is:
//     ///     * `ReadOnly` -  the lock is converted to `Mixed` and the write-lock is added.
//     ///     * `WriteOnly` - the thread-set is updated and the write-lock count is incremented.
//     ///     * `Mixed` - the write-lock is added to the back of the lock-sequence.
//     fn lock_write(&mut self, thread_id: ThreadId) {
//         match self {
//             Self::ReadOnly(locks) => *self = locks.write_lock(thread_id),
//             Self::WriteOnly(locks) => locks.write_lock(thread_id),
//             Self::Mixed(locks) => locks.write_lock(thread_id),
//         }
//     }

//     /// Removes a write lock on `thread_id`, and panics if the lock is not held by the thread.
//     /// If the lock is:
//     ///     * `WriteOnly` - the thread-set is updated and the write-lock count is decremented.
//     ///     * `Mixed` - the write-lock is removed from the front of the lock-sequence. If all
//     ///                 write-locks are removed, the lock is converted to `ReadOnly`. Mixed
//     ///                 locks can never result in returning true because there are at least
//     ///                 two locks in the sequence.
//     fn unlock_write(&mut self, thread_id: ThreadId) -> bool {
//         match self {
//             Self::WriteOnly(locks) => locks.write_unlock(thread_id),
//             Self::Mixed(locks) => {
//                 if locks.write_unlock(thread_id) {
//                     *self = locks.to_read_only();
//                 }
//                 false
//             }
//             Self::ReadOnly(_) => panic!("cannot unlock_write read locks"),
//         }
//     }
// }

// #[derive(Debug)]
// struct ReadOnlyLock {
//     thread_set: ThreadSet,
//     read_locks: [u32; MAX_THREADS],
// }

// impl ReadOnlyLock {
//     /// Increments the read lock count for `thread_id`.
//     fn read_lock(&mut self, thread_id: ThreadId) {
//         self.thread_set.insert(thread_id);
//         self.read_locks[thread_id as usize] += 1;
//     }

//     /// Decrements the read lock count for `thread_id`.
//     /// Returns `true` if the lock is no longer held by any thread.
//     fn read_unlock(&mut self, thread_id: ThreadId) -> bool {
//         self.read_locks[thread_id as usize] =
//             self.read_locks[thread_id as usize].checked_sub(1).unwrap();
//         if self.read_locks[thread_id as usize] == 0 {
//             self.thread_set.remove(thread_id);
//         }

//         self.thread_set.is_empty()
//     }

//     /// Adds a write lock, converting the read lock to a MixedLocks.
//     /// Panics if the thread is not the only thread holding the read lock.
//     fn write_lock(&mut self, thread_id: ThreadId) -> AccountLock {
//         assert_eq!(self.thread_set, ThreadSet::only(thread_id));
//         let read_count = self.read_locks[thread_id as usize];
//         AccountLock::Mixed(MixedLocks {
//             thread_id,
//             total_count: read_count + 1,
//             lock_sequence: vec![
//                 LockSequenceItem::Read(read_count),
//                 LockSequenceItem::Write(1),
//             ]
//             .into(),
//         })
//     }

//     /// ReadOnly locks are schedulable on any thread.
//     fn read_schedulable(&self, num_threads: u8, sequential_queue_limit: u32) -> ThreadSet {
//         let mut thread_set = ThreadSet::any(num_threads);
//         for thread_id in self.thread_set.threads_iter() {
//             if self.read_locks[thread_id as usize] >= sequential_queue_limit {
//                 thread_set.remove(thread_id);
//             }
//         }

//         thread_set
//     }

//     /// Write locks are only schedulable if a single thread holds ReadOnly locks.
//     fn write_schedulable(&self, sequential_queue_limit: u32) -> ThreadSet {
//         self.thread_set
//             .only_one_scheduled()
//             .filter(|thread_id| self.read_locks[*thread_id as usize] < sequential_queue_limit)
//             .map_or_else(ThreadSet::none, ThreadSet::only)
//     }
// }

// #[derive(Debug)]
// struct WriteOnlyLock {
//     thread_id: ThreadId,
//     count: u32,
// }

// impl WriteOnlyLock {
//     /// Increments the write lock count.
//     fn write_lock(&mut self, thread_id: ThreadId) {
//         assert_eq!(self.thread_id, thread_id);
//         self.count += 1;
//     }

//     /// Decrements the write lock count.
//     /// Returns `true` if the lock is no longer held by any thread.
//     fn write_unlock(&mut self, thread_id: ThreadId) -> bool {
//         assert_eq!(self.thread_id, thread_id);
//         self.count = self.count.checked_sub(1).unwrap();
//         self.count == 0
//     }

//     /// Adds a read lock, converting the write lock to a MixedLocks.
//     /// Panics if the thread doesn't already hold the write lock.
//     fn read_lock(&mut self, thread_id: ThreadId) -> AccountLock {
//         assert_eq!(self.thread_id, thread_id);
//         AccountLock::Mixed(MixedLocks {
//             thread_id,
//             total_count: self.count + 1,
//             lock_sequence: vec![
//                 LockSequenceItem::Write(self.count),
//                 LockSequenceItem::Read(1),
//             ]
//             .into(),
//         })
//     }

//     /// Returns `thread_id` as the only schedulable thread.
//     fn schedulable(&self, sequential_queue_limit: u32) -> ThreadSet {
//         if self.count < sequential_queue_limit {
//             ThreadSet::only(self.thread_id)
//         } else {
//             ThreadSet::none()
//         }
//     }
// }

// #[derive(Debug)]
// struct MixedLocks {
//     thread_id: ThreadId,
//     total_count: u32,
//     lock_sequence: VecDeque<LockSequenceItem>,
// }

// impl MixedLocks {
//     /// Adds a write lock on `thread_id`, and panics if the thread doesn't already hold the locks.
//     fn read_lock(&mut self, thread_id: ThreadId) {
//         assert_eq!(self.thread_id, thread_id);
//         if let Some(LockSequenceItem::Read(count)) = self.lock_sequence.back_mut() {
//             *count += 1;
//         } else {
//             self.lock_sequence.push_back(LockSequenceItem::Read(1));
//         }
//     }

//     /// Removes a read lock on `thread_id`, and panics if the thread doesn't already hold the locks.
//     /// Returns `true` if the sequence is now length 1 (i.e. only a write lock is held).
//     fn read_unlock(&mut self, thread_id: ThreadId) -> bool {
//         assert_eq!(self.thread_id, thread_id);
//         if let Some(LockSequenceItem::Read(count)) = self.lock_sequence.front_mut() {
//             *count = count.checked_sub(1).unwrap();
//             if *count == 0 {
//                 self.lock_sequence.pop_front();
//                 self.lock_sequence.len() == 1
//             } else {
//                 false
//             }
//         } else {
//             panic!("read lock not found");
//         }
//     }

//     /// Adds a write lock on `thread_id`, and panics if the thread doesn't already hold the locks.
//     fn write_lock(&mut self, thread_id: ThreadId) {
//         assert_eq!(self.thread_id, thread_id);
//         if let Some(LockSequenceItem::Write(count)) = self.lock_sequence.back_mut() {
//             *count += 1;
//         } else {
//             self.lock_sequence.push_back(LockSequenceItem::Write(1));
//         }
//     }

//     /// Removes a write lock on `thread_id`, and panics if the thread doesn't already hold the locks.
//     /// Returns `true` if the sequence is now length 1 (i.e. only a read lock is held).
//     fn write_unlock(&mut self, thread_id: ThreadId) -> bool {
//         assert_eq!(self.thread_id, thread_id);
//         if let Some(LockSequenceItem::Write(count)) = self.lock_sequence.front_mut() {
//             *count = count.checked_sub(1).unwrap();
//             if *count == 0 {
//                 self.lock_sequence.pop_front();
//                 self.lock_sequence.len() == 1
//             } else {
//                 false
//             }
//         } else {
//             panic!("write lock not found");
//         }
//     }

//     /// Returns `thread_id` as the only schedulable thread.
//     fn schedulable(&self, sequential_queue_limit: u32) -> ThreadSet {
//         if self.total_count < sequential_queue_limit {
//             ThreadSet::only(self.thread_id)
//         } else {
//             ThreadSet::none()
//         }
//     }

//     /// Converts the MixedLocks to a ReadOnlyLock.
//     fn to_read_only(&self) -> AccountLock {
//         assert_eq!(self.lock_sequence.len(), 1);
//         if let LockSequenceItem::Read(count) = self.lock_sequence[0] {
//             AccountLock::new_read_lock(self.thread_id, count)
//         } else {
//             panic!("read lock not found");
//         }
//     }

//     /// Converts the MixedLocks to a WriteOnlyLock.
//     fn to_write_only(&self) -> AccountLock {
//         assert_eq!(self.lock_sequence.len(), 1);
//         if let LockSequenceItem::Write(count) = self.lock_sequence[0] {
//             AccountLock::new_write_lock(self.thread_id, count)
//         } else {
//             panic!("read lock not found");
//         }
//     }
// }

// /// Read and write locks with queued count.
// #[derive(Debug)]
// enum LockSequenceItem {
//     Read(u32),
//     Write(u32),
// }

// impl BitAndAssign for ThreadSet {
//     fn bitand_assign(&mut self, rhs: Self) {
//         self.0 &= rhs.0;
//     }
// }

// impl BitAnd for ThreadSet {
//     type Output = Self;

//     fn bitand(self, rhs: Self) -> Self::Output {
//         Self(self.0 & rhs.0)
//     }
// }

// #[cfg(test)]
// mod tests {
//     use super::*;

//     const TEST_NUM_THREADS: u8 = 4;

//     #[test]
//     #[should_panic]
//     fn too_few_num_threads() {
//         ThreadAwareAccountLocks::new(0, 1);
//     }

//     #[test]
//     #[should_panic]
//     fn too_many_num_threads() {
//         ThreadAwareAccountLocks::new(MAX_THREADS as u8 + 1, 1);
//     }

//     #[test]
//     fn accounts_schedulable_threads() {
//         let pk1 = Pubkey::new_unique();
//         let pk2 = Pubkey::new_unique();
//         let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);

//         // No locks - all threads are schedulable
//         assert_eq!(
//             locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
//             ThreadSet::any(TEST_NUM_THREADS)
//         );

//         // Write lock on pk1 on thread 0 - now only thread 0 is schedulable
//         locks.lock_write_account(&pk1, 0);
//         assert_eq!(
//             locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
//             ThreadSet::only(0)
//         );

//         // Write lock pk2 on thread 0 - can still schedule on thread 0
//         locks.lock_write_account(&pk2, 0);
//         assert_eq!(
//             locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
//             ThreadSet::only(0)
//         );

//         // Move pk2 lock to thread 1 - cannot schedule on any threads
//         locks.unlock_write_account(&pk2, 0);
//         locks.lock_write_account(&pk2, 1);
//         assert_eq!(
//             locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
//             ThreadSet::none()
//         );

//         // Remove pj2 lock, add another lock for pk1 on thread 0 - at `write_queue_limit` so cannot schedule on any threads
//         locks.unlock_write_account(&pk2, 1);
//         locks.lock_write_account(&pk1, 0);
//         assert_eq!(
//             locks.accounts_schedulable_threads([&pk1, &pk2].into_iter(), std::iter::empty()),
//             ThreadSet::none()
//         );
//     }

//     #[test]
//     #[should_panic]
//     fn test_lock_write_account_write_conflict_panic() {
//         let pk1 = Pubkey::new_unique();
//         let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
//         locks.lock_write_account(&pk1, 0);
//         locks.lock_write_account(&pk1, 1);
//     }

//     #[test]
//     #[should_panic]
//     fn test_lock_write_account_read_conflict_panic() {
//         let pk1 = Pubkey::new_unique();
//         let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
//         locks.lock_read_account(&pk1, 0);
//         locks.lock_write_account(&pk1, 1);
//     }

//     #[test]
//     #[should_panic]
//     fn test_unlock_write_account_panic() {
//         let pk1 = Pubkey::new_unique();
//         let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
//         locks.lock_write_account(&pk1, 1);
//         locks.unlock_write_account(&pk1, 0);
//     }

//     #[test]
//     #[should_panic]
//     fn test_lock_read_account_write_conflict_panic() {
//         let pk1 = Pubkey::new_unique();
//         let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
//         locks.lock_write_account(&pk1, 0);
//         locks.lock_read_account(&pk1, 1);
//     }

//     #[test]
//     #[should_panic]
//     fn test_unlock_read_account_panic() {
//         let pk1 = Pubkey::new_unique();
//         let mut locks = ThreadAwareAccountLocks::new(TEST_NUM_THREADS, 2);
//         locks.lock_read_account(&pk1, 0);
//         locks.unlock_read_account(&pk1, 1);
//     }

//     #[test]
//     fn test_account_lock() {
//         let mut lock = AccountLock::new_read_lock(3, 1);
//     }

//     #[test]
//     fn test_thread_set() {
//         let mut thread_set = ThreadSet::none();
//         assert!(thread_set.is_empty());
//         assert_eq!(thread_set.num_threads(), 0);
//         assert_eq!(thread_set.only_one_scheduled(), None);
//         for idx in 0..MAX_THREADS {
//             assert!(!thread_set.contains(idx as ThreadId));
//         }

//         thread_set.insert(4);
//         assert!(!thread_set.is_empty());
//         assert_eq!(thread_set.num_threads(), 1);
//         assert_eq!(thread_set.only_one_scheduled(), Some(4));
//         for idx in 0..MAX_THREADS {
//             assert_eq!(thread_set.contains(idx as ThreadId), idx == 4);
//         }

//         thread_set.insert(2);
//         assert!(!thread_set.is_empty());
//         assert_eq!(thread_set.num_threads(), 2);
//         assert_eq!(thread_set.only_one_scheduled(), None);
//         for idx in 0..MAX_THREADS {
//             assert_eq!(thread_set.contains(idx as ThreadId), idx == 2 || idx == 4);
//         }

//         thread_set.remove(4);
//         assert!(!thread_set.is_empty());
//         assert_eq!(thread_set.num_threads(), 1);
//         assert_eq!(thread_set.only_one_scheduled(), Some(2));
//         for idx in 0..MAX_THREADS {
//             assert_eq!(thread_set.contains(idx as ThreadId), idx == 2);
//         }
//     }
// }
