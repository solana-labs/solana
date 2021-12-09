use {
    crate::{
        accounts_index::{AccountsIndexConfig, IndexValue},
        bucket_map_holder::BucketMapHolder,
        in_mem_accounts_index::InMemAccountsIndex,
        waitable_condvar::WaitableCondvar,
    },
    std::{
        fmt::Debug,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread::{Builder, JoinHandle},
    },
};

/// Manages the lifetime of the background processing threads.
pub struct AccountsIndexStorage<T: IndexValue> {
    _bg_threads: BgThreads,

    pub storage: Arc<BucketMapHolder<T>>,
    pub in_mem: Vec<Arc<InMemAccountsIndex<T>>>,

    /// set_startup(true) creates bg threads which are kept alive until set_startup(false)
    startup_worker_threads: Mutex<Option<BgThreads>>,
}

impl<T: IndexValue> Debug for AccountsIndexStorage<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

/// low-level managing the bg threads
struct BgThreads {
    exit: Arc<AtomicBool>,
    handles: Option<Vec<JoinHandle<()>>>,
    wait: Arc<WaitableCondvar>,
}

impl Drop for BgThreads {
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

impl BgThreads {
    fn new<T: IndexValue>(
        storage: &Arc<BucketMapHolder<T>>,
        in_mem: &[Arc<InMemAccountsIndex<T>>],
        threads: usize,
    ) -> Self {
        // stop signal used for THIS batch of bg threads
        let exit = Arc::new(AtomicBool::default());
        let handles = Some(
            (0..threads)
                .into_iter()
                .map(|_| {
                    let storage_ = Arc::clone(storage);
                    let exit_ = Arc::clone(&exit);
                    let in_mem_ = in_mem.to_vec();

                    // note that using rayon here causes us to exhaust # rayon threads and many tests running in parallel deadlock
                    Builder::new()
                        .name("solana-idx-flusher".to_string())
                        .spawn(move || {
                            storage_.background(exit_, in_mem_);
                        })
                        .unwrap()
                })
                .collect(),
        );

        BgThreads {
            exit,
            handles,
            wait: Arc::clone(&storage.wait_dirty_or_aged),
        }
    }
}

impl<T: IndexValue> AccountsIndexStorage<T> {
    /// startup=true causes:
    ///      in mem to act in a way that flushes to disk asap
    ///      also creates some additional bg threads to facilitate flushing to disk asap
    /// startup=false is 'normal' operation
    pub fn set_startup(&self, value: bool) {
        if value {
            // create some additional bg threads to help get things to the disk index asap
            *self.startup_worker_threads.lock().unwrap() = Some(BgThreads::new(
                &self.storage,
                &self.in_mem,
                Self::num_threads(),
            ));
        }
        self.storage.set_startup(value);
        if !value {
            // transitioning from startup to !startup (ie. steady state)
            // shutdown the bg threads
            *self.startup_worker_threads.lock().unwrap() = None;
            // maybe shrink hashmaps
            self.shrink_to_fit();
        }
    }

    fn shrink_to_fit(&self) {
        self.in_mem.iter().for_each(|mem| mem.shrink_to_fit())
    }

    fn num_threads() -> usize {
        std::cmp::max(2, num_cpus::get() / 4)
    }

    /// allocate BucketMapHolder and InMemAccountsIndex[]
    pub fn new(bins: usize, config: &Option<AccountsIndexConfig>) -> Self {
        let threads = config
            .as_ref()
            .and_then(|config| config.flush_threads)
            .unwrap_or_else(Self::num_threads);

        let storage = Arc::new(BucketMapHolder::new(bins, config, threads));

        let in_mem = (0..bins)
            .into_iter()
            .map(|bin| Arc::new(InMemAccountsIndex::new(&storage, bin)))
            .collect::<Vec<_>>();

        Self {
            _bg_threads: BgThreads::new(&storage, &in_mem, threads),
            storage,
            in_mem,
            startup_worker_threads: Mutex::default(),
        }
    }
}
