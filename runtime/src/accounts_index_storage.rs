use crate::accounts_index::{AccountsIndexConfig, IndexValue};
use crate::bucket_map_holder::BucketMapHolder;
use crate::in_mem_accounts_index::InMemAccountsIndex;
use crate::waitable_condvar::WaitableCondvar;
use std::fmt::Debug;
use std::time::Duration;
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
pub struct AccountsIndexStorage<T: IndexValue> {
    // for managing the bg threads
    exit: Arc<AtomicBool>,
    wait: Arc<WaitableCondvar>,
    handle: Option<JoinHandle<()>>,

    // eventually the backing storage
    storage: Arc<BucketMapHolder<T>>,
    pub in_mem: Vec<Arc<InMemAccountsIndex<T>>>,
}

impl<T: IndexValue> Debug for AccountsIndexStorage<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl<T: IndexValue> Drop for AccountsIndexStorage<T> {
    fn drop(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
        self.wait.notify_all();
        if let Some(x) = self.handle.take() {
            x.join().unwrap()
        }
    }
}

impl<T: IndexValue> AccountsIndexStorage<T> {
    pub fn new(bins: usize, _config: &Option<AccountsIndexConfig>) -> AccountsIndexStorage<T> {
        let storage = Arc::new(BucketMapHolder::new(bins));

        let in_mem = (0..bins)
            .into_iter()
            .map(|bin| Arc::new(InMemAccountsIndex::new(&storage, bin)))
            .collect();

        let storage_ = Arc::clone(&storage);
        let exit = Arc::new(AtomicBool::default());
        let exit_ = Arc::clone(&exit);
        let wait = Arc::new(WaitableCondvar::default());
        let wait_ = Arc::clone(&wait);
        let handle = Some(
            Builder::new()
                .name("solana-index-flusher".to_string())
                .spawn(move || {
                    Self::background(storage_, exit_, wait_);
                })
                .unwrap(),
        );

        Self {
            exit,
            wait,
            handle,
            storage,
            in_mem,
        }
    }

    pub fn storage(&self) -> &Arc<BucketMapHolder<T>> {
        &self.storage
    }

    // intended to execute in a bg thread
    pub fn background(
        storage: Arc<BucketMapHolder<T>>,
        exit: Arc<AtomicBool>,
        wait: Arc<WaitableCondvar>,
    ) {
        loop {
            wait.wait_timeout(Duration::from_millis(10000)); // account index stats every 10 s
            if exit.load(Ordering::Relaxed) {
                break;
            }
            storage.stats.report_stats();
        }
    }
}
