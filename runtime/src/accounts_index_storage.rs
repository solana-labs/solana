use crate::accounts_index::{AccountsIndexConfig, IndexValue};
use crate::bucket_map_holder::BucketMapHolder;
use crate::in_mem_accounts_index::InMemAccountsIndex;
use std::fmt::Debug;
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
        self.storage.wait_dirty_or_aged.notify_all();
        if let Some(handles) = self.handles.take() {
            handles
                .into_iter()
                .for_each(|handle| handle.join().unwrap());
        }
    }
}

impl<T: IndexValue> AccountsIndexStorage<T> {
    pub fn add_worker_threads(existing: &Self, threads: usize) -> Self {
        Self::allocate(
            Arc::clone(&existing.storage),
            existing.in_mem.clone(),
            threads,
        )
    }

    pub fn set_startup(&self, value: bool) {
        self.storage.set_startup(self, value);
    }

    fn allocate(
        storage: Arc<BucketMapHolder<T>>,
        in_mem: Vec<Arc<InMemAccountsIndex<T>>>,
        threads: usize,
    ) -> Self {
        let exit = Arc::new(AtomicBool::default());
        let handles = Some(
            (0..threads)
                .into_iter()
                .map(|_| {
                    let storage_ = Arc::clone(&storage);
                    let exit_ = Arc::clone(&exit);
                    let in_mem_ = in_mem.clone();

                    // note that rayon use here causes us to exhaust # rayon threads and many tests running in parallel deadlock
                    Builder::new()
                        .name("solana-idx-flusher".to_string())
                        .spawn(move || {
                            storage_.background(exit_, in_mem_);
                        })
                        .unwrap()
                })
                .collect(),
        );

        Self {
            exit,
            handles,
            storage,
            in_mem,
        }
    }

    pub fn new(bins: usize, config: &Option<AccountsIndexConfig>) -> Self {
        let num_threads = std::cmp::max(2, num_cpus::get() / 4);
        let threads = config
            .as_ref()
            .and_then(|config| config.flush_threads)
            .unwrap_or(num_threads);

        let storage = Arc::new(BucketMapHolder::new(bins, config, threads));

        let in_mem = (0..bins)
            .into_iter()
            .map(|bin| Arc::new(InMemAccountsIndex::new(&storage, bin)))
            .collect::<Vec<_>>();

        Self::allocate(storage, in_mem, threads)
    }
}
