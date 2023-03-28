use {
    super::thread_set::{ThreadId, ThreadSet, MAX_THREADS},
    std::collections::VecDeque,
};

#[derive(Debug)]
pub(crate) enum ThreadAwareLock {
    ReadOnly(Box<ReadOnlyLock>),
    WriteOnly(WriteOnlyLock),
    Mixed(MixedLocks),
}

impl ThreadAwareLock {
    /// Creates a new `ReadOnly` lock on `thread_id` with `count` read locks.
    pub(crate) fn new_read_lock(thread_id: ThreadId, count: u32) -> Self {
        let mut read_locks = [0; MAX_THREADS];
        read_locks[thread_id] = count;
        Self::ReadOnly(Box::new(ReadOnlyLock {
            thread_set: ThreadSet::only(thread_id),
            read_locks,
        }))
    }

    /// Creates a new `WriteOnly` lock on `thread_id` with `count` write locks.
    pub(crate) fn new_write_lock(thread_id: ThreadId, count: u32) -> Self {
        Self::WriteOnly(WriteOnlyLock { thread_id, count })
    }

    /// Returns set of threads that can be read-scheduled.
    /// If the lock is:
    ///     * `ReadOnly` - the `Any` thread-set is returned.
    ///     * `WriteOnly` - thread-set including `thread_id` is returned.
    ///     * `Mixed` - thread-set including `thread_id` is returned.
    pub(crate) fn read_schedulable(
        &self,
        num_threads: usize,
        sequential_queue_limit: u32,
    ) -> ThreadSet {
        match self {
            Self::ReadOnly(locks) => locks.read_schedulable(num_threads, sequential_queue_limit),
            Self::WriteOnly(locks) => locks.schedulable(sequential_queue_limit),
            Self::Mixed(locks) => locks.schedulable(sequential_queue_limit),
        }
    }

    /// Returns set of threads that can be write-scheduled.
    /// If the lock is:
    ///     * `ReadOnly` - the `Any` thread-set is returned.
    ///     * `WriteOnly` - thread-set including `thread_id` is returned.
    ///     * `Mixed` - thread-set including `thread_id` is returned.
    pub(crate) fn write_schedulable(&self, sequential_queue_limit: u32) -> ThreadSet {
        match self {
            Self::ReadOnly(locks) => locks.write_schedulable(sequential_queue_limit),
            Self::WriteOnly(locks) => locks.schedulable(sequential_queue_limit),
            Self::Mixed(locks) => locks.schedulable(sequential_queue_limit),
        }
    }

    /// Adds a read lock on `thread_id`, and panics if that is not possible due to write-conflict.
    /// If the lock is:
    ///     * `ReadOnly` -  the thread-set is updated and the read-lock count is incremented.
    ///     * `WriteOnly` - the lock is converted to `Mixed` and the read-lock is added.
    ///     * `Mixed` - the read-lock is added to the back of the lock-sequence.
    pub(crate) fn read_lock(mut self, thread_id: ThreadId) -> Self {
        match &mut self {
            Self::ReadOnly(locks) => {
                locks.read_lock(thread_id);
                self
            }
            Self::WriteOnly(locks) => locks.read_lock(thread_id),
            Self::Mixed(locks) => {
                locks.read_lock(thread_id);
                self
            }
        }
    }

    /// Removes a read lock on `thread_id`, and panics if the lock is not held by the thread.
    /// If the lock is:
    ///     * `ReadOnly` -  the thread-set is updated and the read-lock count is decremented.
    ///     * `Mixed` - the read-lock is removed from the front of the lock-sequence. If all
    ///                 read-locks are removed, the lock is converted to `WriteOnly`. Mixed
    ///                 locks can never result in returning true because there are at least
    ///                 two locks in the sequence.
    pub(crate) fn read_unlock(mut self, thread_id: ThreadId) -> Option<Self> {
        match self {
            Self::ReadOnly(ref mut locks) => locks.read_unlock(thread_id).then_some(self),
            Self::Mixed(locks) => Some(locks.read_unlock(thread_id)),
            Self::WriteOnly(_) => panic!("cannot unlock_read write locks"),
        }
    }

    /// Adds a write lock on `thread_id`, and panics if that is not possible due to conflicts.
    /// If the lock is:
    ///     * `ReadOnly` -  the lock is converted to `Mixed` and the write-lock is added.
    ///     * `WriteOnly` - the thread-set is updated and the write-lock count is incremented.
    ///     * `Mixed` - the write-lock is added to the back of the lock-sequence.
    pub(crate) fn write_lock(mut self, thread_id: ThreadId) -> Self {
        match &mut self {
            Self::ReadOnly(locks) => locks.write_lock(thread_id),
            Self::WriteOnly(locks) => {
                locks.write_lock(thread_id);
                self
            }
            Self::Mixed(locks) => {
                locks.write_lock(thread_id);
                self
            }
        }
    }

    /// Removes a write lock on `thread_id`, and panics if the lock is not held by the thread.
    /// If the lock is:
    ///     * `WriteOnly` - the thread-set is updated and the write-lock count is decremented.
    ///     * `Mixed` - the write-lock is removed from the front of the lock-sequence. If all
    ///                 write-locks are removed, the lock is converted to `ReadOnly`. Mixed
    ///                 locks can never result in returning true because there are at least
    ///                 two locks in the sequence.
    pub(crate) fn write_unlock(mut self, thread_id: ThreadId) -> Option<Self> {
        match self {
            Self::WriteOnly(ref mut locks) => locks.write_unlock(thread_id).then_some(self),
            Self::Mixed(locks) => Some(locks.write_unlock(thread_id)),
            Self::ReadOnly(_) => panic!("cannot unlock_write read locks"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ReadOnlyLock {
    thread_set: ThreadSet,
    read_locks: [u32; MAX_THREADS],
}

impl ReadOnlyLock {
    /// Increments the read lock count for `thread_id`.
    fn read_lock(&mut self, thread_id: ThreadId) {
        self.thread_set.insert(thread_id);
        self.read_locks[thread_id] += 1;
    }

    /// Decrements the read lock count for `thread_id`.
    /// Returns `true` if the lock is still held.
    fn read_unlock(&mut self, thread_id: ThreadId) -> bool {
        self.read_locks[thread_id] = self.read_locks[thread_id].checked_sub(1).unwrap();
        if self.read_locks[thread_id] == 0 {
            self.thread_set.remove(thread_id);
        }

        !self.thread_set.is_empty()
    }

    /// Adds a write lock, converting the read lock to a MixedLocks.
    /// Panics if the thread is not the only thread holding the read lock.
    fn write_lock(&mut self, thread_id: ThreadId) -> ThreadAwareLock {
        assert_eq!(self.thread_set, ThreadSet::only(thread_id));
        let read_count = self.read_locks[thread_id];
        ThreadAwareLock::Mixed(MixedLocks {
            thread_id,
            total_count: read_count + 1,
            lock_sequence: vec![
                LockSequenceItem::Read(read_count),
                LockSequenceItem::Write(1),
            ]
            .into(),
        })
    }

    /// ReadOnly locks are schedulable on any thread.
    fn read_schedulable(&self, num_threads: usize, sequential_queue_limit: u32) -> ThreadSet {
        let mut thread_set = ThreadSet::any(num_threads);
        for thread_id in self.thread_set.threads_iter() {
            if self.read_locks[thread_id] >= sequential_queue_limit {
                thread_set.remove(thread_id);
            }
        }

        thread_set
    }

    /// Write locks are only schedulable if a single thread holds ReadOnly locks.
    fn write_schedulable(&self, sequential_queue_limit: u32) -> ThreadSet {
        self.thread_set
            .only_one_scheduled()
            .filter(|thread_id| self.read_locks[*thread_id] < sequential_queue_limit)
            .map_or_else(ThreadSet::none, ThreadSet::only)
    }
}

#[derive(Debug)]
pub(crate) struct WriteOnlyLock {
    thread_id: ThreadId,
    count: u32,
}

impl WriteOnlyLock {
    /// Increments the write lock count.
    fn write_lock(&mut self, thread_id: ThreadId) {
        assert_eq!(self.thread_id, thread_id);
        self.count += 1;
    }

    /// Decrements the write lock count.
    /// Returns `true` if the lock is still held.
    fn write_unlock(&mut self, thread_id: ThreadId) -> bool {
        assert_eq!(self.thread_id, thread_id);
        self.count = self.count.checked_sub(1).unwrap();
        self.count != 0
    }

    /// Adds a read lock, converting the write lock to a MixedLocks.
    /// Panics if the thread doesn't already hold the write lock.
    fn read_lock(&mut self, thread_id: ThreadId) -> ThreadAwareLock {
        assert_eq!(self.thread_id, thread_id);
        ThreadAwareLock::Mixed(MixedLocks {
            thread_id,
            total_count: self.count + 1,
            lock_sequence: vec![
                LockSequenceItem::Write(self.count),
                LockSequenceItem::Read(1),
            ]
            .into(),
        })
    }

    /// Returns `thread_id` as the only schedulable thread.
    fn schedulable(&self, sequential_queue_limit: u32) -> ThreadSet {
        if self.count < sequential_queue_limit {
            ThreadSet::only(self.thread_id)
        } else {
            ThreadSet::none()
        }
    }
}

#[derive(Debug)]
pub(crate) struct MixedLocks {
    thread_id: ThreadId,
    total_count: u32,
    lock_sequence: VecDeque<LockSequenceItem>,
}

impl MixedLocks {
    /// Adds a write lock on `thread_id`, and panics if the thread doesn't already hold the locks.
    fn read_lock(&mut self, thread_id: ThreadId) {
        assert_eq!(self.thread_id, thread_id);
        if let Some(LockSequenceItem::Read(count)) = self.lock_sequence.back_mut() {
            *count += 1;
        } else {
            self.lock_sequence.push_back(LockSequenceItem::Read(1));
        }
    }

    /// Removes a read lock on `thread_id`, and panics if the thread doesn't already hold the locks.
    /// Returns the new lock state - either a WriteOnly lock or a Mixed lock.
    fn read_unlock(mut self, thread_id: ThreadId) -> ThreadAwareLock {
        assert_eq!(self.thread_id, thread_id);
        if let Some(LockSequenceItem::Read(count)) = self.lock_sequence.front_mut() {
            self.total_count = self.total_count.checked_sub(1).unwrap();
            *count = count.checked_sub(1).unwrap();
            if *count == 0 {
                self.lock_sequence.pop_front();
                if self.lock_sequence.len() == 1 {
                    return ThreadAwareLock::new_write_lock(self.thread_id, self.total_count);
                }
            }

            ThreadAwareLock::Mixed(self)
        } else {
            panic!("read lock not found");
        }
    }

    /// Adds a write lock on `thread_id`, and panics if the thread doesn't already hold the locks.
    fn write_lock(&mut self, thread_id: ThreadId) {
        assert_eq!(self.thread_id, thread_id);
        if let Some(LockSequenceItem::Write(count)) = self.lock_sequence.back_mut() {
            *count += 1;
        } else {
            self.lock_sequence.push_back(LockSequenceItem::Write(1));
        }
    }

    /// Removes a write lock on `thread_id`, and panics if the thread doesn't already hold the locks.
    /// Returns the new lock state - either a ReadOnly lock or a Mixed lock.
    fn write_unlock(mut self, thread_id: ThreadId) -> ThreadAwareLock {
        assert_eq!(self.thread_id, thread_id);
        if let Some(LockSequenceItem::Write(count)) = self.lock_sequence.front_mut() {
            self.total_count = self.total_count.checked_sub(1).unwrap();
            *count = count.checked_sub(1).unwrap();
            if *count == 0 {
                self.lock_sequence.pop_front();
                if self.lock_sequence.len() == 1 {
                    return ThreadAwareLock::new_read_lock(self.thread_id, self.total_count);
                }
            }

            ThreadAwareLock::Mixed(self)
        } else {
            panic!("write lock not found");
        }
    }

    /// Returns `thread_id` as the only schedulable thread.
    fn schedulable(&self, sequential_queue_limit: u32) -> ThreadSet {
        if self.total_count < sequential_queue_limit {
            ThreadSet::only(self.thread_id)
        } else {
            ThreadSet::none()
        }
    }

    /// Converts the MixedLocks to a ReadOnlyLock.
    fn to_read_only(&self) -> ThreadAwareLock {
        assert_eq!(self.lock_sequence.len(), 1);
        if let LockSequenceItem::Read(count) = self.lock_sequence[0] {
            ThreadAwareLock::new_read_lock(self.thread_id, count)
        } else {
            panic!("read lock not found");
        }
    }

    /// Converts the MixedLocks to a WriteOnlyLock.
    fn to_write_only(&self) -> ThreadAwareLock {
        assert_eq!(self.lock_sequence.len(), 1);
        if let LockSequenceItem::Write(count) = self.lock_sequence[0] {
            ThreadAwareLock::new_write_lock(self.thread_id, count)
        } else {
            panic!("read lock not found");
        }
    }
}

/// Read and write locks with queued count.
#[derive(Debug)]
enum LockSequenceItem {
    Read(u32),
    Write(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_NUM_THREADS: usize = 8;
    const TEST_SEQ_LIMIT: u32 = 4;

    #[test]
    fn test_read_only_lock() {
        // Single read-lock on thread 0. All threads can read, only thread 0 can write.
        let mut lock = ThreadAwareLock::new_read_lock(0, 1);
        assert!(matches!(lock, ThreadAwareLock::ReadOnly(_)));
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::any(TEST_NUM_THREADS)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::only(0));

        // Add another read-lock on thread 0. All threads can read, only thread 0 can write.
        lock = lock.read_lock(0);
        assert!(matches!(lock, ThreadAwareLock::ReadOnly(_)));
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::any(TEST_NUM_THREADS)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::only(0));

        // Remove
        println!("{lock:?}");
        lock = lock.read_unlock(0).unwrap();

        // Add a read-lock on thread 1. Any threads can read, no threads can write.
        lock = lock.read_lock(1);
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::any(TEST_NUM_THREADS)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::none());

        // Remove
        lock = lock.read_unlock(1).unwrap();
        assert!(lock.read_unlock(0).is_none()); // No more locks
    }

    #[test]
    fn test_write_only_lock() {
        // Single write-lock on thread 2. Only thread 2 can read or write.
        let mut lock = ThreadAwareLock::new_write_lock(2, 1);
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::only(2)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::only(2));

        // Add another write-lock on thread 2. Only thread 2 can read or write.
        lock = lock.write_lock(2);
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::only(2)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::only(2));

        // Remove
        lock = lock.write_unlock(2).unwrap();
        assert!(lock.write_unlock(2).is_none()); // No more locks
    }

    #[test]
    fn test_read_to_mixed() {
        let mut lock = ThreadAwareLock::new_read_lock(0, 1);
        lock = lock.write_lock(0);
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::only(0)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::only(0));

        lock = lock.read_unlock(0).unwrap();
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::only(0)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::only(0));
        assert!(lock.write_unlock(0).is_none());
    }

    #[test]
    fn test_write_to_mixed() {
        let mut lock = ThreadAwareLock::new_write_lock(0, 1);
        lock = lock.read_lock(0);
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::only(0)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::only(0));

        lock = lock.write_unlock(0).unwrap();
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::any(TEST_NUM_THREADS)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::only(0));
        assert!(lock.read_unlock(0).is_none());
    }

    #[test]
    fn test_read_only_schedulable_limit() {
        let lock = ThreadAwareLock::new_read_lock(0, TEST_SEQ_LIMIT);
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::any(TEST_NUM_THREADS) - ThreadSet::only(0)
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::none());
    }

    #[test]
    fn test_write_only_schedulable_limit() {
        let lock = ThreadAwareLock::new_write_lock(0, TEST_SEQ_LIMIT);
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::none()
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::none());
    }

    #[test]
    fn test_mixed_schedulable_limit() {
        let mut lock = ThreadAwareLock::new_write_lock(0, TEST_SEQ_LIMIT - 1);
        lock = lock.read_lock(0);
        assert_eq!(
            lock.read_schedulable(TEST_NUM_THREADS, TEST_SEQ_LIMIT),
            ThreadSet::none()
        );
        assert_eq!(lock.write_schedulable(TEST_SEQ_LIMIT), ThreadSet::none());
    }

    #[test]
    #[should_panic]
    fn test_read_only_locks_invalid_lock() {
        let lock = ThreadAwareLock::new_read_lock(0, 1);
        lock.write_lock(1);
    }

    #[test]
    #[should_panic]
    fn test_write_only_locks_invalid_read_lock() {
        let lock = ThreadAwareLock::new_write_lock(0, 1);
        lock.read_lock(1);
    }

    #[test]
    #[should_panic]
    fn test_write_only_locks_invalid_write_lock() {
        let lock = ThreadAwareLock::new_write_lock(0, 1);
        lock.write_lock(1);
    }

    #[test]
    #[should_panic]
    fn test_mixed_locks_invalid_read_lock() {
        let lock = ThreadAwareLock::Mixed(MixedLocks {
            thread_id: 0,
            total_count: 2,
            lock_sequence: vec![LockSequenceItem::Write(1), LockSequenceItem::Read(1)].into(),
        });
        lock.read_lock(1);
    }

    #[test]
    #[should_panic]
    fn test_mixed_locks_invalid_write_lock() {
        let lock = ThreadAwareLock::Mixed(MixedLocks {
            thread_id: 0,
            total_count: 2,
            lock_sequence: vec![LockSequenceItem::Write(1), LockSequenceItem::Read(1)].into(),
        });
        lock.write_lock(1);
    }

    #[test]
    #[should_panic]
    fn test_mixed_locks_invalid_read_unlock() {
        let lock = ThreadAwareLock::Mixed(MixedLocks {
            thread_id: 0,
            total_count: 2,
            lock_sequence: vec![LockSequenceItem::Write(1), LockSequenceItem::Read(1)].into(),
        });
        lock.read_unlock(0);
    }

    #[test]
    #[should_panic]
    fn test_mixed_locks_invalid_write_unlock() {
        let lock = ThreadAwareLock::Mixed(MixedLocks {
            thread_id: 0,
            total_count: 2,
            lock_sequence: vec![LockSequenceItem::Read(1), LockSequenceItem::Write(1)].into(),
        });
        lock.write_unlock(0);
    }

    #[test]
    #[should_panic]
    fn test_mixed_locks_invalid_unlock() {
        let lock = ThreadAwareLock::Mixed(MixedLocks {
            thread_id: 0,
            total_count: 2,
            lock_sequence: vec![LockSequenceItem::Read(1), LockSequenceItem::Write(1)].into(),
        });
        lock.write_unlock(1);
    }
}
