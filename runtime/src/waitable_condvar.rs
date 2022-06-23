use std::{
    sync::{Condvar, Mutex},
    time::Duration,
};

// encapsulate complications of unneeded mutex and Condvar to give us event behavior of wait and notify
// this will likely be wrapped in an arc somehow
#[derive(Default, Debug)]
pub struct WaitableCondvar {
    pub mutex: Mutex<()>,
    pub event: Condvar,
}

impl WaitableCondvar {
    /// wake up all threads waiting on this event
    pub fn notify_all(&self) {
        self.event.notify_all();
    }
    /// wake up one thread waiting on this event
    pub fn notify_one(&self) {
        self.event.notify_one();
    }
    /// wait on the event
    /// return true if timed out, false if event triggered
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let lock = self.mutex.lock().unwrap();
        let res = self.event.wait_timeout(lock, timeout).unwrap();
        if res.1.timed_out() {
            return true;
        }
        false
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        std::{
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc,
            },
            thread::Builder,
        },
    };
    #[ignore]
    #[test]
    fn test_waitable_condvar() {
        let data = Arc::new(AtomicBool::new(false));
        let data_ = data.clone();
        let cv = Arc::new(WaitableCondvar::default());
        let cv2 = Arc::new(WaitableCondvar::default());
        let cv_ = cv.clone();
        let cv2_ = cv2.clone();
        let cv2__ = cv2.clone();
        // several passes to check re-notification and drop one of the
        let passes = 3;
        let handle = Builder::new().spawn(move || {
            for _pass in 0..passes {
                let mut notified = false;
                while cv2_.wait_timeout(Duration::from_millis(1)) {
                    if !notified && data_.load(Ordering::Relaxed) {
                        notified = true;
                        cv_.notify_all();
                    }
                }
                assert!(data_.swap(false, Ordering::Relaxed));
            }
        });
        // just wait, but 1 less pass - verifies that notify_all works with multiple and with 1
        let handle2 = Builder::new().spawn(move || {
            for _pass in 0..(passes - 1) {
                assert!(!cv2__.wait_timeout(Duration::from_millis(10000))); // long enough to not be intermittent, short enough to fail if we really don't get notified
            }
        });
        for _pass in 0..passes {
            assert!(cv.wait_timeout(Duration::from_millis(1)));
            assert!(!data.swap(true, Ordering::Relaxed));
            assert!(!cv.wait_timeout(Duration::from_millis(10000))); // should barely wait, but don't want intermittent
            cv2.notify_all();
        }
        assert!(handle.unwrap().join().is_ok());
        assert!(handle2.unwrap().join().is_ok());
    }
}
