use std::{
    fmt::Display,
    ops::{BitAnd, BitAndAssign, Sub, SubAssign},
};

pub(crate) const MAX_THREADS: usize = 64;
pub(crate) type ThreadId = usize; // 0..MAX_THREADS - 1

/// A bit-set of threads an account is scheduled or can be scheduled for.
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct ThreadSet(u64);

impl ThreadSet {
    #[inline(always)]
    pub fn none() -> Self {
        Self(0)
    }

    #[inline(always)]
    pub fn any(num_threads: usize) -> Self {
        debug_assert!(num_threads <= MAX_THREADS);
        Self((1 << num_threads) - 1)
    }

    #[inline(always)]
    pub fn only(thread_id: ThreadId) -> Self {
        debug_assert!(thread_id < MAX_THREADS);
        Self(1 << thread_id)
    }

    #[inline(always)]
    pub fn num_threads(&self) -> u32 {
        self.0.count_ones()
    }

    #[inline(always)]
    pub fn only_one_scheduled(&self) -> Option<ThreadId> {
        (self.num_threads() == 1).then(|| self.0.trailing_zeros() as ThreadId)
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

impl Sub for ThreadSet {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 & !rhs.0)
    }
}

impl SubAssign for ThreadSet {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 &= !rhs.0;
    }
}

#[cfg(test)]
static_assertions::assert_eq_size!(u64, ThreadSet); // Display fmt needs to change if size changes.
impl Display for ThreadSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ThreadSet({:#064b})", self.0)
    }
}

// Manual implementation of Debug to show the bit-set as a binary string.
impl std::fmt::Debug for ThreadSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

#[cfg(test)]
mod tests {
    use {super::*, itertools::Itertools};

    #[test]
    fn test_thread_set_none() {
        let thread_set = ThreadSet::none();
        assert!(thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 0);
        assert_eq!(thread_set.only_one_scheduled(), None);
        for idx in 0..MAX_THREADS {
            assert!(!thread_set.contains(idx as ThreadId));
        }
        assert!(thread_set.threads_iter().collect_vec().is_empty());
    }

    #[test]
    fn test_thread_set_only() {
        let thread_set = ThreadSet::only(3);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 1);
        assert_eq!(thread_set.only_one_scheduled(), Some(3));
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx as ThreadId), idx == 3);
        }
        assert_eq!(thread_set.threads_iter().collect_vec(), vec![3]);
    }

    #[test]
    fn test_thread_set_any() {
        let thread_set = ThreadSet::any(4);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 4);
        assert_eq!(thread_set.only_one_scheduled(), None);
        for idx in 0..MAX_THREADS {
            assert_eq!(thread_set.contains(idx as ThreadId), idx < 4);
        }
        assert_eq!(thread_set.threads_iter().collect_vec(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_thread_set_insert() {
        let mut thread_set = ThreadSet::none();
        thread_set.insert(3);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 1);
        assert_eq!(thread_set.only_one_scheduled(), Some(3));
        assert_eq!(thread_set.threads_iter().collect_vec(), vec![3]);

        thread_set.insert(2);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 2);
        assert_eq!(thread_set.only_one_scheduled(), None);
        assert_eq!(thread_set.threads_iter().collect_vec(), vec![2, 3]);

        // Re-insert is allowed.
        thread_set.insert(3);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 2);
        assert_eq!(thread_set.only_one_scheduled(), None);
        assert_eq!(thread_set.threads_iter().collect_vec(), vec![2, 3]);
    }

    #[test]
    fn test_thread_set_remove() {
        let mut thread_set = ThreadSet::any(4);
        thread_set.remove(1);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 3);
        assert_eq!(thread_set.only_one_scheduled(), None);
        assert_eq!(thread_set.threads_iter().collect_vec(), vec![0, 2, 3]);

        thread_set.remove(2);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 2);
        assert_eq!(thread_set.only_one_scheduled(), None);
        assert_eq!(thread_set.threads_iter().collect_vec(), vec![0, 3]);

        // Removing a non-existent thread is allowed.
        thread_set.remove(2);
        assert!(!thread_set.is_empty());
        assert_eq!(thread_set.num_threads(), 2);
        assert_eq!(thread_set.only_one_scheduled(), None);
        assert_eq!(thread_set.threads_iter().collect_vec(), vec![0, 3]);
    }

    #[test]
    fn test_thread_set_and() {
        let thread_set1 = {
            let mut thread_set = ThreadSet::none();
            thread_set.insert(1);
            thread_set.insert(3);
            thread_set
        };

        let thread_set2 = {
            let mut thread_set = ThreadSet::none();
            thread_set.insert(2);
            thread_set.insert(3);
            thread_set
        };

        let thread_set3 = thread_set1 & thread_set2;
        assert_eq!(thread_set3.threads_iter().collect_vec(), vec![3]);
    }
}
