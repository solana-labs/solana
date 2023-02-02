//! A simple wrapper for state shared between threads, that can also be waited on.

use {
    parking_lot::{Condvar, Mutex},
    std::{sync::Arc, time::Instant},
};

/// A convenient wrapper, hiding some of the boilerplate required to deal with a shared state, that
/// also has an attached update notification mechanism.
pub struct SharedWrapper<T> {
    inner: Mutex<T>,
    condvar: Condvar,
}

impl<T> SharedWrapper<T> {
    pub fn new(inner: T) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(inner),
            condvar: Condvar::new(),
        })
    }

    /// Updates the inner state, notifying everybody waiting, after the change is performed.
    pub fn update_notify_all(&self, f: impl FnOnce(&mut T)) {
        let mut inner = self.inner.lock();
        f(&mut inner);
        self.condvar.notify_all();
    }

    /// Runs provided function, and returns the value it returned.  Or waits for the inner state to
    /// change and tries again.
    pub fn check_and_wait<Res>(&self, mut f: impl FnMut(&T) -> Option<Res>) -> Res {
        let mut inner = self.inner.lock();
        loop {
            match f(&inner) {
                Some(result) => return result,
                None => self.condvar.wait(&mut inner),
            }
        }
    }

    /// Runs provided function, and returns the value it returned.  Or waits for the inner state to
    /// change and tries again.
    ///
    /// `None` is returned when the desired state is not reached by the deadline.
    pub fn check_and_wait_until<Res>(
        &self,
        deadline: Instant,
        mut f: impl FnMut(&T) -> Option<Res>,
    ) -> Option<Res> {
        let mut inner = self.inner.lock();
        loop {
            match f(&inner) {
                Some(result) => return Some(result),
                None => {
                    let res = self.condvar.wait_until(&mut inner, deadline);
                    if res.timed_out() {
                        return None;
                    }
                }
            }
        }
    }
}

impl<T: Default> Default for SharedWrapper<T> {
    fn default() -> Self {
        Self {
            inner: Mutex::new(T::default()),
            condvar: Condvar::new(),
        }
    }
}
