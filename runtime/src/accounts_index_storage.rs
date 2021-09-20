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
    handles: Option<Vec<JoinHandle<()>>>,

    // eventually the backing storage
    pub storage: Arc<BucketMapHolder<T>>,
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
        if let Some(handles) = self.handles.take() {
            handles
                .into_iter()
                .for_each(|handle| handle.join().unwrap());
        }
    }
}

impl<T: IndexValue> AccountsIndexStorage<T> {
    pub fn new(bins: usize, config: &Option<AccountsIndexConfig>) -> AccountsIndexStorage<T> {
        let storage = Arc::new(BucketMapHolder::new(bins, config));

        let in_mem = (0..bins)
            .into_iter()
            .map(|bin| Arc::new(InMemAccountsIndex::new(&storage, bin)))
            .collect::<Vec<_>>();

        const DEFAULT_THREADS: usize = 1; // soon, this will be a cpu calculation
        let threads = config
            .as_ref()
            .and_then(|config| config.flush_threads)
            .unwrap_or(DEFAULT_THREADS);

        let exit = Arc::new(AtomicBool::default());
        let wait = Arc::new(WaitableCondvar::default());
        let handles = Some(
            (0..threads)
                .into_iter()
                .map(|_| {
                    let storage_ = Arc::clone(&storage);
                    let exit_ = Arc::clone(&exit);
                    let wait_ = Arc::clone(&wait);
                    let in_mem_ = in_mem.clone();

                    // note that rayon use here causes us to exhaust # rayon threads and many tests running in parallel deadlock
                    Builder::new()
                        .name("solana-idx-flusher".to_string())
                        .spawn(move || {
                            Self::background(storage_, exit_, wait_, in_mem_);
                        })
                        .unwrap()
                })
                .collect(),
        );

        Self {
            exit,
            wait,
            handles,
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
        in_mem: Vec<Arc<InMemAccountsIndex<T>>>,
    ) {
        loop {
            // this will transition to waits and thread throttling
            wait.wait_timeout(Duration::from_millis(10000));
            if exit.load(Ordering::Relaxed) {
                break;
            }

            storage.stats.active_threads.fetch_add(1, Ordering::Relaxed);
            for bucket in &in_mem {
                bucket.flush();
            }

            storage.stats.report_stats();
            storage.stats.active_threads.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
