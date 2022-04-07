//! keep track of areas of the validator that are currently active
use {
    crate::waitable_condvar::WaitableCondvar,
    std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    },
};

#[derive(Debug, Default)]
pub struct ForegroundRequestsResources {
    /// # of calls to set_busy(true) - # of calls to set_busy(false)
    busy_count: AtomicUsize,
    /// mechanism for waiting and waking up
    wait: Arc<WaitableCondvar>,
}

/// sole purpose is to handle 'drop' so that count is decremented when self is dropped
pub struct ForegroundRequestsResourcesGuard<'a> {
    requests: &'a ForegroundRequestsResources,
}

impl<'a> Drop for ForegroundRequestsResourcesGuard<'a> {
    fn drop(&mut self) {
        self.requests.set_busy(false);
    }
}

impl ForegroundRequestsResources {
    #[must_use]
    /// inc the count and create a stack object to dec the count on drop
    pub fn activate(&self) -> ForegroundRequestsResourcesGuard<'_> {
        self.set_busy(true);
        ForegroundRequestsResourcesGuard { requests: self }
    }

    /// true if busy_count > 0, indicating bg processes should sleep
    pub fn get_busy(&self) -> bool {
        self.busy_count.load(Ordering::Acquire) > 0
    }

    /// if fg requested resources, sleep a bit
    /// could also sleep until request is satisfied
    pub fn possibly_sleep(&self) {
        if self.get_busy() {
            self.wait.wait_timeout(Duration::from_millis(1));
        }
    }

    /// specify that foreground is busy or not
    fn set_busy(&self, busy: bool) {
        if busy {
            self.busy_count.fetch_add(1, Ordering::Release);
        } else {
            let result = self.busy_count.fetch_sub(1, Ordering::Release);
            if result == 1 {
                self.wait.notify_all();
            }
        }
    }
}
