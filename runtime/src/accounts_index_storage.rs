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
    exit: Arc<AtomicBool>,

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
        can_advance_age: bool,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        // stop signal used for THIS batch of bg threads
        let local_exit = Arc::new(AtomicBool::default());
        let handles = Some(
            (0..threads)
                .map(|idx| {
                    // the first thread we start is special
                    let can_advance_age = can_advance_age && idx == 0;
                    let storage_ = Arc::clone(storage);
                    let local_exit_ = Arc::clone(&local_exit);
                    let system_exit_ = Arc::clone(exit);
                    let in_mem_ = in_mem.to_vec();

                    // note that using rayon here causes us to exhaust # rayon threads and many tests running in parallel deadlock
                    Builder::new()
                        .name(format!("solIdxFlusher{idx:02}"))
                        .spawn(move || {
                            storage_.background(
                                vec![local_exit_, system_exit_],
                                in_mem_,
                                can_advance_age,
                            );
                        })
                        .unwrap()
                })
                .collect(),
        );

        BgThreads {
            exit: local_exit,
            handles,
            wait: Arc::clone(&storage.wait_dirty_or_aged),
        }
    }
}

/// modes the system can be in
pub enum Startup {
    /// not startup, but steady state execution
    Normal,
    /// startup (not steady state execution)
    /// requesting 'startup'-like behavior where in-mem acct idx items are flushed asap
    Startup,
    /// startup (not steady state execution)
    /// but also requesting additional threads to be running to flush the acct idx to disk asap
    /// The idea is that the best perf to ssds will be with multiple threads,
    ///  but during steady state, we can't allocate as many threads because we'd starve the rest of the system.
    StartupWithExtraThreads,
}

impl<T: IndexValue> AccountsIndexStorage<T> {
    /// startup=true causes:
    ///      in mem to act in a way that flushes to disk asap
    ///      also creates some additional bg threads to facilitate flushing to disk asap
    /// startup=false is 'normal' operation
    pub fn set_startup(&self, startup: Startup) {
        let value = !matches!(startup, Startup::Normal);
        if matches!(startup, Startup::StartupWithExtraThreads) {
            // create some additional bg threads to help get things to the disk index asap
            *self.startup_worker_threads.lock().unwrap() = Some(BgThreads::new(
                &self.storage,
                &self.in_mem,
                Self::num_threads(),
                false, // cannot advance age from any of these threads
                &self.exit,
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

    /// estimate how many items are still needing to be flushed to the disk cache.
    pub fn get_startup_remaining_items_to_flush_estimate(&self) -> usize {
        self.storage
            .disk
            .as_ref()
            .map(|_| self.storage.stats.get_remaining_items_to_flush_estimate())
            .unwrap_or_default()
    }

    fn shrink_to_fit(&self) {
        self.in_mem.iter().for_each(|mem| mem.shrink_to_fit())
    }

    fn num_threads() -> usize {
        std::cmp::max(2, num_cpus::get() / 4)
    }

    /// allocate BucketMapHolder and InMemAccountsIndex[]
    pub fn new(bins: usize, config: &Option<AccountsIndexConfig>, exit: &Arc<AtomicBool>) -> Self {
        let threads = config
            .as_ref()
            .and_then(|config| config.flush_threads)
            .unwrap_or_else(Self::num_threads);

        let storage = Arc::new(BucketMapHolder::new(bins, config, threads));

        let in_mem = (0..bins)
            .map(|bin| Arc::new(InMemAccountsIndex::new(&storage, bin)))
            .collect::<Vec<_>>();

        Self {
            _bg_threads: BgThreads::new(&storage, &in_mem, threads, true, exit),
            storage,
            in_mem,
            startup_worker_threads: Mutex::default(),
            exit: Arc::clone(exit),
        }
    }
}
