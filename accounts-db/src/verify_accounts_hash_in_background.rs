//! at startup, verify accounts hash in the background
use {
    crate::waitable_condvar::WaitableCondvar,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread::JoinHandle,
        time::Duration,
    },
};

#[derive(Debug)]
pub struct VerifyAccountsHashInBackground {
    /// true when verification has completed or never had to run in background
    pub verified: Arc<AtomicBool>,
    /// enable waiting for verification to become complete
    complete: Arc<WaitableCondvar>,
    /// thread doing verification
    thread: Mutex<Option<JoinHandle<bool>>>,
    /// set when background thread has completed
    background_completed: Arc<AtomicBool>,
}

impl Default for VerifyAccountsHashInBackground {
    fn default() -> Self {
        // initialize, expecting possible background verification to be started
        Self {
            complete: Arc::default(),
            // with default initialization, 'verified' is false
            verified: Arc::new(AtomicBool::new(false)),
            // no thread to start with
            thread: Mutex::new(None::<JoinHandle<bool>>),
            background_completed: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl VerifyAccountsHashInBackground {
    /// start the bg thread to do the verification
    pub fn start(&self, start: impl FnOnce() -> JoinHandle<bool>) {
        // note that we're not verified before
        self.verified.store(false, Ordering::Release);
        *self.thread.lock().unwrap() = Some(start());
    }

    /// notify that the bg process has completed
    pub fn background_finished(&self) {
        self.complete.notify_all();
        self.background_completed.store(true, Ordering::Release);
    }

    /// notify that verification was completed successfully
    /// This can occur because it completed in the background
    /// or if the verification was run in the foreground.
    pub fn verification_complete(&self) {
        self.verified.store(true, Ordering::Release);
    }

    /// block until bg process is complete
    pub fn wait_for_complete(&self) {
        // just now completing
        let mut lock = self.thread.lock().unwrap();
        if lock.is_none() {
            return; // nothing to do
        }
        let result = lock.take().unwrap().join().unwrap();
        if !result {
            panic!("initial background accounts hash verification failed: {result}");
        }
        // we never have to check again
        self.verification_complete();
    }

    /// return true if bg hash verification is complete
    /// return false if bg hash verification has not completed yet
    /// if hash verification failed, a panic will occur
    pub fn check_complete(&self) -> bool {
        if self.verified.load(Ordering::Acquire) {
            // already completed
            return true;
        }
        if self.complete.wait_timeout(Duration::default())
            && !self.background_completed.load(Ordering::Acquire)
        {
            // timed out, so not complete
            false
        } else {
            // Did not time out, so thread finished. Join it.
            self.wait_for_complete();
            true
        }
    }
}

#[cfg(test)]
pub mod tests {
    use {super::*, std::thread::Builder};

    #[test]
    fn test_default() {
        let def = VerifyAccountsHashInBackground::default();
        assert!(!def.check_complete());
        assert!(!def.verified.load(Ordering::Acquire));
        assert!(def.thread.lock().unwrap().is_none());
        def.verification_complete();
        assert!(def.check_complete());
    }

    fn start_thread_and_return(
        verify: &Arc<VerifyAccountsHashInBackground>,
        result: bool,
        action: impl FnOnce() + Send + 'static,
    ) {
        assert!(!verify.check_complete());
        let verify_ = Arc::clone(verify);
        verify.start(|| {
            Builder::new()
                .name("solBgHashVerfy".to_string())
                .spawn(move || {
                    // should have been marked not complete before thread started
                    assert!(!verify_.check_complete());
                    action();
                    verify_.background_finished();
                    result
                })
                .unwrap()
        });
    }

    #[test]
    fn test_real() {
        solana_logger::setup();
        let verify = Arc::new(VerifyAccountsHashInBackground::default());
        start_thread_and_return(&verify, true, || {});
        verify.wait_for_complete();
        assert!(verify.check_complete());
    }

    #[test]
    #[should_panic(expected = "initial background accounts hash verification failed")]
    fn test_panic() {
        let verify = Arc::new(VerifyAccountsHashInBackground::default());
        start_thread_and_return(&verify, false, || {});
        verify.wait_for_complete();
        assert!(!verify.check_complete());
    }

    #[test]
    fn test_long_running() {
        solana_logger::setup();
        let verify = Arc::new(VerifyAccountsHashInBackground::default());
        let finish = Arc::new(AtomicBool::default());
        let finish_ = finish.clone();
        start_thread_and_return(&verify, true, move || {
            // busy wait until atomic is set
            while !finish_.load(Ordering::Relaxed) {}
        });
        assert!(!verify.check_complete());
        finish.store(true, Ordering::Relaxed);
        verify.wait_for_complete();
        assert!(verify.check_complete());
    }
}
