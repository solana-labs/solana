use crate::accounts_index::IsCached;
use crate::bucket_map_holder::BucketMapHolder;
use crate::waitable_condvar::WaitableCondvar;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{Builder, JoinHandle},
};

// eventually hold the bucket map
// Also manages the lifetime of the background processing threads.
//  When this instance is dropped, it will drop the bucket map and cleanup
//  and it will stop all the background threads and join them.

#[derive(Debug, Default)]
pub struct AccountsIndexStorage<T: IsCached> {
    // for managing the bg threads
    exit: Arc<AtomicBool>,
    wait: Arc<WaitableCondvar>,
    handle: Option<JoinHandle<()>>,

    // eventually the backing storage
    storage: Arc<BucketMapHolder<T>>,
}

impl<T: IsCached> Drop for AccountsIndexStorage<T> {
    fn drop(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
        self.wait.notify_all();
        if let Some(x) = self.handle.take() {
            x.join().unwrap()
        }
    }
}

impl<T: IsCached> AccountsIndexStorage<T> {
    pub fn new() -> AccountsIndexStorage<T> {
        let storage = Arc::new(BucketMapHolder::new());
        let storage_ = storage.clone();
        let exit = Arc::new(AtomicBool::default());
        let exit_ = exit.clone();
        let wait = Arc::new(WaitableCondvar::default());
        let wait_ = wait.clone();
        let handle = Some(
            Builder::new()
                .name("solana-index-flusher".to_string())
                .spawn(move || {
                    storage_.background(exit_, wait_);
                })
                .unwrap(),
        );

        Self {
            exit,
            wait,
            handle,
            storage,
        }
    }

    pub fn storage(&self) -> &Arc<BucketMapHolder<T>> {
        &self.storage
    }
}
