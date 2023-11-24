//! Transaction scheduling code.
//!
//! This crate implements two solana-runtime traits (`InstalledScheduler` and
//! `InstalledSchedulerPool`) to provide concrete transaction scheduling implementation (including
//! executing txes and committing tx results).
//!
//! At highest level, this crate takes `SanitizedTransaction`s via its `schedule_execution()` and
//! commits any side-effects (i.e. on-chain state changes) into `Bank`s via `solana-ledger`'s
//! helper fun called `execute_batch()`.

use {
    crossbeam_channel::{
        bounded, never, select_biased, unbounded, Receiver, RecvTimeoutError, Sender, TryRecvError,
    },
    log::*,
    rand::{thread_rng, Rng},
    solana_ledger::blockstore_processor::{
        execute_batch, TransactionBatchWithIndexes, TransactionStatusSender,
    },
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{
        bank::Bank,
        installed_scheduler_pool::{
            DefaultScheduleExecutionArg, InstalledScheduler, InstalledSchedulerPool,
            InstalledSchedulerPoolArc, ResultWithTimings, ScheduleExecutionArg, SchedulerId,
            SchedulingContext, WaitReason, WithTransactionAndIndex,
        },
        prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_scheduler::{SchedulingMode, WithSchedulingMode},
    solana_sdk::{
        pubkey::Pubkey,
        slot_history::Slot,
        transaction::{Result, SanitizedTransaction},
    },
    solana_vote::vote_sender_types::ReplayVoteSender,
    std::{
        cell::UnsafeCell,
        collections::HashMap,
        fmt::Debug,
        marker::PhantomData,
        sync::{atomic::AtomicUsize, Arc, Mutex, RwLock, RwLockReadGuard, Weak},
        thread::{sleep, JoinHandle},
        time::{Duration, SystemTime},
    },
};

type UniqueWeight = u64;

#[derive(Debug, Default)]
pub struct Tasks {
    blocked_task_queue: std::collections::BTreeMap<UniqueWeight, (TaskInQueue, RequestedUsage)>,
}

// SchedulerPool must be accessed via dyn by solana-runtime code, because of its internal fields'
// types (currently TransactionStatusSender; also, PohRecorder in the future) aren't available
// there...
#[derive(Debug)]
pub struct SchedulerPool<
    T: SpawnableScheduler<TH, SEA>,
    TH: Handler<SEA>,
    SEA: ScheduleExecutionArg,
> {
    schedulers: Mutex<Vec<Box<T>>>,
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    // weak_self could be elided by changing InstalledScheduler::take_scheduler()'s receiver to
    // Arc<Self> from &Self, because SchedulerPool is used as in the form of Arc<SchedulerPool>
    // almost always. But, this would cause wasted and noisy Arc::clone()'s at every call sites.
    //
    // Alternatively, `impl InstalledScheduler for Arc<SchedulerPool>` approach could be explored
    // but it entails its own problems due to rustc's coherence and necessitated newtype with the
    // type graph of InstalledScheduler being quite elaborate.
    //
    // After these considerations, this weak_self approach is chosen at the cost of some additional
    // memory increase.
    weak_self: Weak<Self>,
    // prune schedulers, stop idling scheduler's threads, sanity check on the
    // address book after scheduler is returned.
    watchdog_sender: Sender<Weak<RwLock<ThreadManager<TH, SEA>>>>,
    watchdog_thread: JoinHandle<()>,
    _phantom: PhantomData<(T, TH, SEA)>,
}

pub type DefaultSchedulerPool = SchedulerPool<
    PooledScheduler<DefaultTransactionHandler, DefaultScheduleExecutionArg>,
    DefaultTransactionHandler,
    DefaultScheduleExecutionArg,
>;

struct WatchedThreadManager<TH, SEA>
where
    TH: Handler<SEA>,
    SEA: ScheduleExecutionArg,
{
    thread_manager: Weak<RwLock<ThreadManager<TH, SEA>>>,
    tick: u64,
    updated_at: SystemTime,
}

impl<TH, SEA> WatchedThreadManager<TH, SEA>
where
    TH: Handler<SEA>,
    SEA: ScheduleExecutionArg,
{
    fn new(thread_manager: Weak<RwLock<ThreadManager<TH, SEA>>>) -> Self {
        Self {
            thread_manager,
            tick: 0,
            updated_at: SystemTime::now(),
        }
    }

    fn update_tick_to_retain(&mut self) -> bool {
        let Some(thread_manager) = self.thread_manager.upgrade() else {
            return false;
        };
        let Some(tid) = thread_manager
            .read()
            .unwrap()
            .scheduler_thread_and_tid
            .as_ref()
            .map(|&(_, tid)| tid)
        else {
            self.tick = 0;
            self.updated_at = SystemTime::now();
            return true;
        };

        let pid = std::process::id();
        let task = (procfs::process::Process::new(pid.try_into().unwrap())
            .unwrap()
            .task_from_tid(tid)
            .unwrap());
        let stat = task.stat().unwrap();
        let current_tick = stat.utime + stat.stime;
        if current_tick > self.tick {
            self.tick = current_tick;
            self.updated_at = SystemTime::now();
        } else if let Ok(elapsed) = self.updated_at.elapsed() {
            if elapsed > Duration::from_secs(10) {
                const BITS_PER_HEX_DIGIT: usize = 4;
                let mut thread_manager = thread_manager.write().unwrap();
                info!(
                    "[sch_{:0width$x}]: watchdog: update_tick_to_retain(): stopping thread manager ({tid}/{} <= {}/{:?})...",
                    thread_manager.scheduler_id,
                    current_tick,
                    self.tick,
                    elapsed,
                    width = SchedulerId::BITS as usize / BITS_PER_HEX_DIGIT,
                );
                thread_manager.stop_threads();
                self.tick = 0;
                self.updated_at = SystemTime::now();
            }
        }

        true
    }
}

impl<T, TH, SEA> SchedulerPool<T, TH, SEA>
where
    T: SpawnableScheduler<TH, SEA>,
    TH: Handler<SEA>,
    SEA: ScheduleExecutionArg,
{
    pub fn new(
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> Arc<Self> {
        let (scheduler_pool_sender, scheduler_pool_receiver) = bounded(1);
        let (watchdog_sender, watchdog_receiver) = unbounded();

        let watchdog_main_loop = || {
            move || {
                let scheduler_pool: Arc<Self> = scheduler_pool_receiver.recv().unwrap();
                drop(scheduler_pool_receiver);

                let mut watched_thread_managers: Vec<WatchedThreadManager<TH, SEA>> = vec![];

                'outer: loop {
                    let mut schedulers = scheduler_pool.schedulers.lock().unwrap();
                    let pre_schedulers_len = schedulers.len();
                    schedulers.retain_mut(|scheduler| scheduler.should_retain_in_pool());
                    let post_schedulers_len = schedulers.len();
                    drop(schedulers);

                    let pre_retain_len = watched_thread_managers.len();
                    watched_thread_managers
                        .retain_mut(|thread_manager| thread_manager.update_tick_to_retain());

                    let pre_push_len = watched_thread_managers.len();
                    'inner: loop {
                        match watchdog_receiver.recv_timeout(Duration::from_secs(2)) {
                            Ok(thread_manager) => watched_thread_managers
                                .push(WatchedThreadManager::new(thread_manager)),
                            Err(RecvTimeoutError::Disconnected) => break 'outer,
                            Err(RecvTimeoutError::Timeout) => break 'inner,
                        }
                    }

                    info!(
                        "watchdog: schedulers in the pool: {} => {}, watched thread managers: {} => {} => {}",
                        pre_schedulers_len,
                        post_schedulers_len,
                        pre_retain_len,
                        pre_push_len,
                        watched_thread_managers.len(),
                    );
                }
            }
        };

        let watchdog_thread = std::thread::Builder::new()
            .name("solScWatchdog".to_owned())
            .spawn(watchdog_main_loop())
            .unwrap();

        let scheduler_pool = Arc::new_cyclic(|weak_self| Self {
            schedulers: Mutex::new(vec![]),
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
            weak_self: weak_self.clone(),
            watchdog_thread,
            watchdog_sender,
            _phantom: PhantomData,
        });
        scheduler_pool_sender.send(scheduler_pool.clone()).unwrap();
        scheduler_pool
    }

    pub fn new_dyn(
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> InstalledSchedulerPoolArc<SEA> {
        Self::new(
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
        )
    }

    // See a comment at the weak_self field for justification of this.
    pub fn self_arc(&self) -> Arc<Self> {
        self.weak_self
            .upgrade()
            .expect("self-referencing Arc-ed pool")
    }

    pub fn return_scheduler(&self, mut scheduler: Box<T>) {
        //assert!(!scheduler.has_context());

        self.schedulers
            .lock()
            .expect("not poisoned")
            .push(scheduler);
    }

    pub fn do_take_scheduler(&self, context: SchedulingContext) -> Box<T> {
        // pop is intentional for filo, expecting relatively warmed-up scheduler due to having been
        // returned recently
        if let Some(mut scheduler) = self.schedulers.lock().expect("not poisoned").pop() {
            scheduler.replace_context(context);
            scheduler
        } else {
            Box::new(T::spawn(self.self_arc(), context, TH::create(self)))
        }
    }

    fn register_to_watchdog(&self, thread_manager: Weak<RwLock<ThreadManager<TH, SEA>>>) {
        self.watchdog_sender.send(thread_manager).unwrap();
    }
}

impl<T, TH, SEA> InstalledSchedulerPool<SEA> for SchedulerPool<T, TH, SEA>
where
    T: SpawnableScheduler<TH, SEA>,
    TH: Handler<SEA>,
    SEA: ScheduleExecutionArg,
{
    fn take_scheduler(&self, context: SchedulingContext) -> Box<dyn InstalledScheduler<SEA>> {
        self.do_take_scheduler(context)
    }
}

pub trait Handler<SEA: ScheduleExecutionArg>:
    Send + Sync + Debug + Sized + Clone + 'static
{
    fn create<T: SpawnableScheduler<Self, SEA>>(pool: &SchedulerPool<T, Self, SEA>) -> Self;

    fn handle<T: SpawnableScheduler<Self, SEA>>(
        &self,
        result: &mut Result<()>,
        timings: &mut ExecuteTimings,
        bank: &Arc<Bank>,
        transaction: &SanitizedTransaction,
        index: usize,
        pool: &SchedulerPool<T, Self, SEA>,
    );
}

#[derive(Debug, Clone)]
pub struct DefaultTransactionHandler;

impl<SEA: ScheduleExecutionArg> Handler<SEA> for DefaultTransactionHandler {
    fn create<T: SpawnableScheduler<Self, SEA>>(_pool: &SchedulerPool<T, Self, SEA>) -> Self {
        Self
    }

    fn handle<T: SpawnableScheduler<Self, SEA>>(
        &self,
        result: &mut Result<()>,
        timings: &mut ExecuteTimings,
        bank: &Arc<Bank>,
        transaction: &SanitizedTransaction,
        index: usize,
        pool: &SchedulerPool<T, Self, SEA>,
    ) {
        // scheduler must properly prevent conflicting tx executions, so locking isn't needed
        // here
        let batch = bank.prepare_unlocked_batch_from_single_tx(transaction);
        let batch_with_indexes = TransactionBatchWithIndexes {
            batch,
            transaction_indexes: vec![index],
        };

        *result = execute_batch(
            &batch_with_indexes,
            bank,
            pool.transaction_status_sender.as_ref(),
            pool.replay_vote_sender.as_ref(),
            timings,
            pool.log_messages_bytes_limit,
            &pool.prioritization_fee_cache,
        );
    }
}

type UsageCount = usize;
const SOLE_USE_COUNT: UsageCount = 1;

#[derive(Clone, Debug)]
enum LockStatus {
    Succeded,
    Failed,
}

pub type TaskInQueue = Arc<Task>;

#[derive(Debug)]
pub struct LockAttemptsInCell(UnsafeCell<Vec<LockAttempt>>);

#[derive(Debug)]
struct Task {
    unique_weight: UniqueWeight,
    tx: SanitizedTransaction, // actually should be Bundle
    lock_attempts: LockAttemptsInCell,
    contention_count: std::sync::atomic::AtomicUsize,
    uncontended: std::sync::atomic::AtomicUsize,
}

impl Task {
    fn new_for_queue(
        unique_weight: UniqueWeight,
        tx: SanitizedTransaction,
        lock_attempts: Vec<LockAttempt>,
    ) -> TaskInQueue {
        TaskInQueue::new(Self {
            unique_weight,
            tx,
            lock_attempts: LockAttemptsInCell(UnsafeCell::new(lock_attempts)),
            uncontended: Default::default(),
            contention_count: Default::default(),
        })
    }

    fn index_with_pages(this: &TaskInQueue) {
        for lock_attempt in &*this.lock_attempts_mut() {
            let mut page = lock_attempt.target_page_mut();

            page.blocked_task_queue.insert_task(this.clone(), lock_attempt.requested_usage);
        }
    }

    fn lock_attempts_mut(&self) -> &mut Vec<LockAttempt> {
        unsafe { &mut *self.lock_attempts.0.get() }
    }

    pub fn currently_contended(&self) -> bool {
        self.uncontended.load(std::sync::atomic::Ordering::SeqCst) == 1
    }

    fn mark_as_contended(&self) {
        self.uncontended
            .store(1, std::sync::atomic::Ordering::SeqCst)
    }

    fn mark_as_uncontended(&self) {
        assert!(self.currently_contended());
        self.uncontended
            .store(2, std::sync::atomic::Ordering::SeqCst)
    }

    fn task_index(&self) -> usize {
        (UniqueWeight::max_value() - self.unique_weight) as usize
    }
}

#[derive(Debug)]
pub struct LockAttempt {
    page: PageRc,
    status: LockStatus,
    requested_usage: RequestedUsage,
}

impl PageRc {
    fn as_mut(&self) -> &mut Page {
        unsafe { &mut *self.0 .0.get() }
    }
}

impl LockAttempt {
    pub fn new(page: PageRc, requested_usage: RequestedUsage) -> Self {
        Self {
            page,
            status: LockStatus::Succeded,
            requested_usage,
        }
    }

    pub fn clone_for_test(&self) -> Self {
        Self {
            page: self.page.clone(),
            status: LockStatus::Succeded,
            requested_usage: self.requested_usage,
        }
    }

    fn target_page_mut(&self) -> &mut Page {
        self.page.as_mut()
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum Usage {
    Unused,
    Readonly(UsageCount),
    Writable,
}

impl Usage {
    fn renew(requested_usage: RequestedUsage) -> Self {
        match requested_usage {
            RequestedUsage::Readonly => Usage::Readonly(SOLE_USE_COUNT),
            RequestedUsage::Writable => Usage::Writable,
        }
    }

    fn unused() -> Self {
        Usage::Unused
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestedUsage {
    Readonly,
    Writable,
}

#[derive(Debug)]
pub struct Page {
    current_usage: Usage,
    blocked_task_queue: Tasks,
    blocked_write_requesting_task_ids: std::collections::BTreeSet<UniqueWeight>,
}

impl Page {
    fn new(address: &Pubkey, current_usage: Usage) -> Self {
        Self {
            current_usage,
            blocked_task_queue: Default::default(),
            blocked_write_requesting_task_ids: Default::default(),
        }
    }
}

impl Tasks {
    pub fn insert_task(&mut self, task: TaskInQueue, requested_usage: RequestedUsage) {
        let pre_existed = self.blocked_task_queue.insert(task.unique_weight, (task, requested_usage));
        assert!(pre_existed.is_none());
    }

    fn remove_task(&mut self, u: &UniqueWeight) {
        let removed_entry = self.blocked_task_queue.remove(u);
        assert!(removed_entry.is_some());
    }

    fn heaviest_task_cursor(&self) -> impl Iterator<Item = &TaskInQueue> {
        self.blocked_task_queue.values().rev().map(|(task, _ru)| task)
    }

    fn heaviest_writing_task_weight(&self) -> Option<UniqueWeight> {
        self.blocked_task_queue.values().rev().filter(|(task, ru)| ru == &RequestedUsage::Writable).next().map(|(task, _ru)| task.unique_weight)
    }

    pub fn heaviest_weight(&mut self) -> Option<UniqueWeight> {
        self.blocked_task_queue.last_entry().map(|j| *j.key())
    }

    fn reindex(&mut self, should_remove: bool, uq: &UniqueWeight) -> Option<TaskInQueue> {
        if should_remove {
            self.remove_task(uq);
        }

        self.heaviest_task_cursor()
            .find(|task| task.currently_contended())
            .cloned()
    }
}

// use UnsafeCell
type PageRcInner = Arc<UnsafeCell<Page>>;
static_assertions::const_assert_eq!(std::mem::size_of::<std::cell::UnsafeCell<Page>>(), 64);

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct PageRc(by_address::ByAddress<PageRcInner>);
unsafe impl Send for PageRc {}
unsafe impl Sync for PageRc {}

unsafe impl Sync for LockAttemptsInCell {}
type WeightedTaskQueue = std::collections::BTreeMap<UniqueWeight, TaskInQueue>;

type AddressMap = dashmap::DashMap<Pubkey, PageRc>;
#[derive(Default, Debug)]
pub struct AddressBook {
    book: AddressMap,
}

impl AddressBook {
    pub fn load(&self, address: Pubkey) -> PageRc {
        PageRc::clone(&self.book.entry(address).or_insert_with(|| {
            PageRc(by_address::ByAddress(PageRcInner::new(UnsafeCell::new(
                Page::new(&address, Usage::unused()),
            ))))
        }))
    }

    pub fn page_count(&self) -> usize {
        self.book.len()
    }
}

#[derive(Debug)]
pub struct ExecutionEnvironment {
    task: TaskInQueue,
    result_with_timings: ResultWithTimings,
    finish_time: Option<SystemTime>,
    slot: Slot,
    thx: usize,
    execution_us: u64,
    execution_cpu_us: u128,
}

impl ExecutionEnvironment {
    fn new_boxed(task: TaskInQueue) -> Box<Self> {
        Box::new(Self {
            task,
            result_with_timings: (Ok(()), Default::default()),
            finish_time: None,
            slot: 0,
            thx: 0,
            execution_us: 0,
            execution_cpu_us: 0,
        })
    }
}

// Currently, simplest possible implementation (i.e. single-threaded)
// this will be replaced with more proper implementation...
// not usable at all, especially for mainnet-beta
#[derive(Debug)]
pub struct PooledScheduler<TH: Handler<SEA>, SEA: ScheduleExecutionArg> {
    completed_result_with_timings: Option<ResultWithTimings>,
    thread_manager: Arc<RwLock<ThreadManager<TH, SEA>>>,
    address_book: AddressBook,
    pooled_at: SystemTime,
}

#[derive(Debug)]
struct WeakSchedulingContext {
    mode: SchedulingMode,
    bank: Weak<Bank>,
}

impl WeakSchedulingContext {
    fn new(context: SchedulingContext) -> Self {
        Self {
            mode: context.mode(),
            bank: Arc::downgrade(context.bank()),
        }
    }

    fn upgrade(&self) -> Option<SchedulingContext> {
        self.bank
            .upgrade()
            .map(|bank| SchedulingContext::new(self.mode, bank))
    }
}

#[derive(Debug)]
struct ThreadManager<TH: Handler<SEA>, SEA: ScheduleExecutionArg> {
    scheduler_id: SchedulerId,
    pool: Arc<SchedulerPool<PooledScheduler<TH, SEA>, TH, SEA>>,
    context: WeakSchedulingContext,
    scheduler_thread_and_tid: Option<(JoinHandle<ResultWithTimings>, i32)>,
    handler_threads: Vec<JoinHandle<()>>,
    drop_thread: Option<JoinHandle<()>>,
    handler: TH,
    schedulrable_transaction_sender: Sender<ChainedChannel<TaskInQueue, ControlFrame>>,
    schedulable_transaction_receiver: Receiver<ChainedChannel<TaskInQueue, ControlFrame>>,
    result_sender: Sender<ResultWithTimings>,
    result_receiver: Receiver<ResultWithTimings>,
    handler_count: usize,
    session_result_with_timings: Option<ResultWithTimings>,
}

impl<TH: Handler<SEA>, SEA: ScheduleExecutionArg> PooledScheduler<TH, SEA> {
    pub fn do_spawn(
        pool: Arc<SchedulerPool<Self, TH, SEA>>,
        initial_context: SchedulingContext,
        handler: TH,
    ) -> Self {
        let handler_count = std::env::var("SOLANA_UNIFIED_SCHEDULER_HANDLER_COUNT")
            .unwrap_or(format!("{}", 8))
            .parse::<usize>()
            .unwrap();
        let scheduler = Self {
            completed_result_with_timings: None,
            thread_manager: Arc::new(RwLock::new(ThreadManager::<TH, SEA>::new(
                initial_context,
                handler,
                pool.clone(),
                handler_count,
            ))),
            address_book: AddressBook::default(),
            pooled_at: SystemTime::now(),
        };
        pool.register_to_watchdog(Arc::downgrade(&scheduler.thread_manager));

        scheduler
    }

    #[must_use]
    fn ensure_thread_manager_started(&self) -> RwLockReadGuard<'_, ThreadManager<TH, SEA>> {
        loop {
            let r = self.thread_manager.read().unwrap();
            if r.is_active() {
                debug!("ensure_thread_manager_started(): is already active...");
                return r;
            } else {
                debug!("ensure_thread_manager_started(): will start threads...");
                drop(r);
                let mut w = self.thread_manager.write().unwrap();
                w.start_threads();
                drop(w);
            }
        }
    }

    fn pooled_now(&mut self) {
        self.pooled_at = SystemTime::now();
    }

    fn pooled_since(&self) -> Option<Duration> {
        self.pooled_at.elapsed().ok()
    }

    fn stop_thread_manager(&mut self) {
        debug!("stop_thread_manager()");
        self.thread_manager.write().unwrap().stop_threads();
    }
}

type ChannelAndPayload<T1, T2> = (Receiver<ChainedChannel<T1, T2>>, T2);

trait WithChannelAndPayload<T1, T2>: Send + Sync {
    fn channel_and_payload(self: Box<Self>) -> ChannelAndPayload<T1, T2>;
}

struct ChannelAndPayloadTuple<T1, T2>(ChannelAndPayload<T1, T2>);

impl<T1: Send + Sync, T2: Send + Sync> WithChannelAndPayload<T1, T2>
    for ChannelAndPayloadTuple<T1, T2>
{
    fn channel_and_payload(mut self: Box<Self>) -> ChannelAndPayload<T1, T2> {
        self.0
    }
}

enum ChainedChannel<T1, T2> {
    Payload(T1),
    ChannelWithPayload(Box<dyn WithChannelAndPayload<T1, T2>>),
}

enum ControlFrame {
    StartSession(SchedulingContext),
    EndSession,
}

impl<T1: Send + Sync + 'static, T2: Send + Sync + 'static> ChainedChannel<T1, T2> {
    fn new_channel(receiver: Receiver<Self>, sender: T2) -> Self {
        Self::ChannelWithPayload(Box::new(ChannelAndPayloadTuple((receiver, sender))))
    }
}

impl<TH, SEA> ThreadManager<TH, SEA>
where
    TH: Handler<SEA>,
    SEA: ScheduleExecutionArg,
{
    fn new(
        initial_context: SchedulingContext,
        handler: TH,
        pool: Arc<SchedulerPool<PooledScheduler<TH, SEA>, TH, SEA>>,
        handler_count: usize,
    ) -> Self {
        let (schedulrable_transaction_sender, schedulable_transaction_receiver) = unbounded();
        let (result_sender, result_receiver) = unbounded();

        let mut thread_manager = Self {
            scheduler_id: thread_rng().gen::<SchedulerId>(),
            schedulrable_transaction_sender,
            schedulable_transaction_receiver,
            result_sender,
            result_receiver,
            context: WeakSchedulingContext::new(initial_context),
            scheduler_thread_and_tid: None,
            drop_thread: None,
            handler_threads: Vec::with_capacity(handler_count),
            handler_count,
            handler,
            pool,
            session_result_with_timings: None,
        };
        // needs to start threads immediately, because the bank in initial_context can be dropped
        // anytime.
        thread_manager.start_threads();
        thread_manager
    }

    fn is_active(&self) -> bool {
        self.scheduler_thread_and_tid.is_some()
    }

    fn update_result_with_timings(
        (session_result, session_timings): &mut ResultWithTimings,
        msg: &ExecutionEnvironment,
    ) {
        match &msg.result_with_timings.0 {
            Ok(()) => {}
            Err(e) => *session_result = Err(e.clone()),
        }
        session_timings.accumulate(&msg.result_with_timings.1);
    }

    fn receive_scheduled_transaction(
        handler: &TH,
        bank: &Arc<Bank>,
        ee: &mut Box<ExecutionEnvironment>,
        pool: &Arc<SchedulerPool<PooledScheduler<TH, SEA>, TH, SEA>>,
    ) {
        use solana_measure::measure::Measure;
        let (mut wall_time, cpu_time) = (
            Measure::start("process_message_time"),
            cpu_time::ThreadTime::now(),
        );
        debug!("handling task at {:?}", std::thread::current());
        TH::handle(
            handler,
            &mut ee.result_with_timings.0,
            &mut ee.result_with_timings.1,
            bank,
            &ee.task.tx,
            ee.task.task_index(),
            pool,
        );
        ee.slot = bank.slot();
        ee.finish_time = Some(SystemTime::now());
        ee.execution_cpu_us = cpu_time.elapsed().as_micros();
        // make wall time is longer than cpu time, always
        wall_time.stop();
        ee.execution_us = wall_time.as_us();
    }

    fn propagate_context(
        blocked_transaction_sessioned_sender: &mut Sender<
            ChainedChannel<TaskInQueue, ControlFrame>,
        >,
        context: SchedulingContext,
        handler_count: usize,
    ) {
        let (next_blocked_transaction_sessioned_sender, blocked_transaction_sessioned_receiver) =
            unbounded();
        for _ in (0..handler_count) {
            blocked_transaction_sessioned_sender
                .send(ChainedChannel::new_channel(
                    blocked_transaction_sessioned_receiver.clone(),
                    ControlFrame::StartSession(context.clone()),
                ))
                .unwrap();
        }
        drop(std::mem::replace(
            blocked_transaction_sessioned_sender,
            next_blocked_transaction_sessioned_sender,
        ));
    }

    fn active_context(&self) -> Option<SchedulingContext> {
        self.context.upgrade()
    }

    fn start_threads(&mut self) {
        if self.is_active() {
            // this can't be promoted to panic! as read => write upgrade isn't completely
            // race-free in ensure_thread_manager_started()...
            warn!("start_threads(): already started");
            return;
        }
        let context = self
            .active_context()
            .expect("start_threads(): stale scheduler....");
        debug!("start_threads(): doing now");

        let send_metrics = std::env::var("SOLANA_TRANSACTION_TIMINGS").is_ok();

        let (blocked_transaction_sessioned_sender, blocked_transaction_sessioned_receiver) =
            unbounded::<ChainedChannel<TaskInQueue, ControlFrame>>();
        let (idle_transaction_sender, idle_transaction_receiver) = unbounded::<TaskInQueue>();
        let (handled_blocked_transaction_sender, handled_blocked_transaction_receiver) =
            unbounded::<Box<ExecutionEnvironment>>();
        let (handled_idle_transaction_sender, handled_idle_transaction_receiver) =
            unbounded::<Box<ExecutionEnvironment>>();
        let (drop_sender, drop_receiver) = unbounded::<Box<ExecutionEnvironment>>();
        let handler_count = self.handler_count;
        let scheduler_id = self.scheduler_id;
        let mut slot = context.bank().slot();
        let (tid_sender, tid_receiver) = bounded(1);

        let scheduler_main_loop = || {
            let result_sender = self.result_sender.clone();
            let mut schedulable_transaction_receiver =
                self.schedulable_transaction_receiver.clone();
            let mut blocked_transaction_sessioned_sender =
                blocked_transaction_sessioned_sender.clone();
            let mut result_with_timings = self
                .session_result_with_timings
                .take()
                .or(Some((Ok(()), Default::default())));

            move || {
                trace!(
                    "solScheduler thread is started at: {:?}",
                    std::thread::current()
                );
                tid_sender.send(unsafe { libc::gettid() }).unwrap();

                let mut will_end_session = false;
                let mut will_end_thread = false;
                let mut state_machine = SchedulingStateMachine::default();
                let mut log_interval_counter = 0;
                // hint compiler about inline[never] and unlikely?
                macro_rules! log_scheduler {
                    ($a:tt) => {
                        const BITS_PER_HEX_DIGIT: usize = 4;
                        info!(
                            "[sch_{:0width$x}]: slot: {}[{}]({}/{}): state_machine(({}(+{})=>{})/{}|{}/{}) channels(<{} >{}+{} <{}+{})",
                            scheduler_id, slot, ($a), (if will_end_thread {"T"} else {"-"}), (if will_end_session {"S"} else {"-"}),
                            state_machine.active_task_count(), state_machine.retryable_task_count(), state_machine.handled_task_count(),
                            state_machine.total_task_count(),
                            state_machine.reschedule_count(),
                            state_machine.rescheduled_task_count(),
                            schedulable_transaction_receiver.len(),
                            blocked_transaction_sessioned_sender.len(), idle_transaction_sender.len(),
                            handled_blocked_transaction_receiver.len(), handled_idle_transaction_receiver.len(),
                            width = SchedulerId::BITS as usize / BITS_PER_HEX_DIGIT,
                        );
                    };
                    () => {
                        if log_interval_counter == 0 {
                            log_scheduler!("started ");
                        } else if log_interval_counter % 1000 == 0 {
                            log_scheduler!("interval");
                        }
                        log_interval_counter += 1;
                    };
                };

                while !will_end_thread {
                    while !(state_machine.is_empty() && (will_end_session || will_end_thread)) {
                        select_biased! {
                            recv(handled_blocked_transaction_receiver) -> execution_environment => {
                                log_scheduler!();
                                let mut execution_environment = execution_environment.unwrap();
                                Self::update_result_with_timings(result_with_timings.as_mut().unwrap(), &execution_environment);
                                state_machine.deschedule_task(&execution_environment);
                                drop_sender.send(execution_environment).unwrap();
                            },
                            recv(schedulable_transaction_receiver) -> m => {
                                if let Ok(mm) = m {
                                    match mm {
                                        ChainedChannel::Payload(payload) => {
                                            log_scheduler!();
                                            if let Some(ee) = state_machine.schedule_new_task(payload) {
                                                idle_transaction_sender
                                                    .send(ee)
                                                    .unwrap();
                                            }
                                        }
                                        ChainedChannel::ChannelWithPayload(new_channel) => {
                                            let control_frame;
                                            (schedulable_transaction_receiver, control_frame) = new_channel.channel_and_payload();
                                            match control_frame {
                                                ControlFrame::StartSession(context) => {
                                                    slot = context.bank().slot();
                                                    log_scheduler!();
                                                    Self::propagate_context(&mut blocked_transaction_sessioned_sender, context, handler_count);
                                                }
                                                ControlFrame::EndSession => {
                                                    will_end_session = true;
                                                    log_scheduler!("S:ending");
                                                }
                                            }
                                        }
                                    };
                                } else {
                                    assert!(!will_end_thread);
                                    schedulable_transaction_receiver = never();
                                    will_end_thread = true;
                                    log_scheduler!("T:ending");
                                };
                            },
                            recv(handled_idle_transaction_receiver) -> execution_environment => {
                                log_scheduler!();
                                let mut execution_environment = execution_environment.unwrap();
                                Self::update_result_with_timings(result_with_timings.as_mut().unwrap(), &execution_environment);
                                state_machine.deschedule_task(&execution_environment);
                                drop_sender.send(execution_environment).unwrap();
                            },
                        };

                        if let Some(ee) = state_machine.schedule_retryable_task() {
                            blocked_transaction_sessioned_sender
                                .send(ChainedChannel::Payload(ee))
                                .unwrap();
                        }

                        while !(state_machine.is_empty() && (will_end_session || will_end_thread)) {
                            log_scheduler!();

                            if let Ok(mut execution_environment) =
                                handled_blocked_transaction_receiver.try_recv()
                            {
                                Self::update_result_with_timings(
                                    result_with_timings.as_mut().unwrap(),
                                    &execution_environment,
                                );
                                state_machine.deschedule_task(&mut execution_environment);
                                drop_sender.send(execution_environment).unwrap();
                            } else if let Ok(mm) = schedulable_transaction_receiver.try_recv() {
                                match mm {
                                    ChainedChannel::Payload(payload) => {
                                        if let Some(ee) = state_machine.schedule_new_task(payload) {
                                            idle_transaction_sender.send(ee).unwrap();
                                        }
                                    }
                                    ChainedChannel::ChannelWithPayload(new_channel) => {
                                        let control_frame;
                                        (schedulable_transaction_receiver, control_frame) =
                                            new_channel.channel_and_payload();
                                        match control_frame {
                                            ControlFrame::StartSession(context) => {
                                                slot = context.bank().slot();
                                                Self::propagate_context(
                                                    &mut blocked_transaction_sessioned_sender,
                                                    context,
                                                    handler_count,
                                                );
                                            }
                                            ControlFrame::EndSession => {
                                                debug!(
                                                    "scheduler_main_loop: will_end_session = true"
                                                );
                                                will_end_session = true;
                                                log_scheduler!("S:ending");
                                            }
                                        }
                                    }
                                };
                            } else if let Some(ee) = state_machine.schedule_retryable_task() {
                                blocked_transaction_sessioned_sender
                                    .send(ChainedChannel::Payload(ee))
                                    .unwrap();
                            } else if let Ok(mut execution_environment) =
                                handled_idle_transaction_receiver.try_recv()
                            {
                                Self::update_result_with_timings(
                                    result_with_timings.as_mut().unwrap(),
                                    &execution_environment,
                                );
                                state_machine.deschedule_task(&mut execution_environment);
                                drop_sender.send(execution_environment).unwrap();
                            } else {
                                break;
                            }
                        }
                    }

                    if will_end_session {
                        // or should also consider will_end_thread?
                        log_scheduler!("S:ended ");
                        (state_machine, log_interval_counter) = <_>::default();
                        result_sender
                            .send(
                                result_with_timings
                                    .replace((Ok(()), Default::default()))
                                    .unwrap(),
                            )
                            .unwrap();
                        will_end_session = false;
                    }
                }
                log_scheduler!("T:ended ");

                let res = result_with_timings.take().unwrap();
                trace!(
                    "solScheduler thread is ended at: {:?}",
                    std::thread::current()
                );
                res
            }
        };

        let handler_main_loop = |thx| {
            let pool = self.pool.clone();
            let handler = self.handler.clone();
            let mut bank = context.bank().clone();
            let mut blocked_transaction_sessioned_receiver =
                blocked_transaction_sessioned_receiver.clone();
            let mut idle_transaction_receiver = idle_transaction_receiver.clone();
            let handled_blocked_transaction_sender = handled_blocked_transaction_sender.clone();
            let handled_idle_transaction_sender = handled_idle_transaction_sender.clone();

            move || {
                trace!(
                    "solScHandler{:02} thread is started at: {:?}",
                    thx,
                    std::thread::current()
                );
                loop {
                    let (mut m, was_blocked) = select_biased! {
                        recv(blocked_transaction_sessioned_receiver) -> m => {
                            let Ok(mm) = m else { break };

                            match mm {
                                ChainedChannel::Payload(payload) => {
                                    (payload, true)
                                }
                                ChainedChannel::ChannelWithPayload(new_channel) => {
                                    let control_frame;
                                    (blocked_transaction_sessioned_receiver, control_frame) = new_channel.channel_and_payload();
                                    match control_frame {
                                        ControlFrame::StartSession(new_context) => {
                                            bank = new_context.bank().clone();
                                        },
                                        ControlFrame::EndSession => unreachable!(),
                                    }
                                    continue;
                                }
                            }
                        },
                        recv(idle_transaction_receiver) -> m => {
                            let Ok(mm) = m else {
                                idle_transaction_receiver = never();
                                continue;
                            };

                            (mm, false)
                        },
                    };
                    let mut m = ExecutionEnvironment::new_boxed(m);

                    m.thx = thx;
                    Self::receive_scheduled_transaction(&handler, &bank, &mut m, &pool);

                    if was_blocked {
                        handled_blocked_transaction_sender.send(m).unwrap();
                    } else {
                        handled_idle_transaction_sender.send(m).unwrap();
                    }
                }
                trace!(
                    "solScHandler{:02} thread is ended at: {:?}",
                    thx,
                    std::thread::current()
                );
            }
        };

        let drop_main_loop = || {
            move || 'outer: loop {
                'inner: loop {
                    match drop_receiver.try_recv() {
                        Ok(ee) => {
                            if send_metrics {
                                use solana_runtime::transaction_priority_details::GetTransactionPriorityDetails;

                                let sig = ee.task.tx.signature().to_string();

                                solana_metrics::datapoint_info_at!(
                                    ee.finish_time.unwrap(),
                                    "transaction_timings",
                                    ("slot", ee.slot, i64),
                                    ("index", ee.task.task_index(), i64),
                                    ("thread", format!("solScExLane{:02}", ee.thx), String),
                                    ("signature", &sig, String),
                                    (
                                        "account_locks_in_json",
                                        serde_json::to_string(
                                            &ee.task.tx.get_account_locks_unchecked()
                                        )
                                        .unwrap(),
                                        String
                                    ),
                                    ("status", format!("{:?}", ee.result_with_timings.0), String),
                                    ("duration", ee.execution_us, i64),
                                    ("cpu_duration", ee.execution_cpu_us, i64),
                                    ("compute_units", 0 /*ee.cu*/, i64),
                                    (
                                        "priority",
                                        ee.task
                                            .tx
                                            .get_transaction_priority_details(false)
                                            .map(|d| d.priority)
                                            .unwrap_or_default(),
                                        i64
                                    ),
                                );
                            }
                            drop(ee);
                        }
                        Err(TryRecvError::Disconnected) => break 'outer,
                        Err(TryRecvError::Empty) => break 'inner,
                    }
                }
                sleep(Duration::from_millis(40));
            }
        };

        self.scheduler_thread_and_tid = Some((
            std::thread::Builder::new()
                .name("solScheduler".to_owned())
                .spawn(scheduler_main_loop())
                .unwrap(),
            tid_receiver.recv().unwrap(),
        ));

        self.drop_thread = Some(
            std::thread::Builder::new()
                .name("solScDrop".to_owned())
                .spawn(drop_main_loop())
                .unwrap(),
        );

        self.handler_threads = (0..handler_count)
            .map({
                |thx| {
                    std::thread::Builder::new()
                        .name(format!("solScHandler{:02}", thx))
                        .spawn(handler_main_loop(thx))
                        .unwrap()
                }
            })
            .collect();
    }

    fn stop_threads(&mut self) {
        if !self.is_active() {
            warn!("stop_threads(): already not active anymore...");
            return;
        }
        debug!(
            "stop_threads(): stopping threads by {:?}",
            std::thread::current()
        );

        (
            self.schedulrable_transaction_sender,
            self.schedulable_transaction_receiver,
        ) = unbounded();
        let result_with_timings = self
            .scheduler_thread_and_tid
            .take()
            .unwrap()
            .0
            .join()
            .unwrap();
        let () = self.drop_thread.take().unwrap().join().unwrap();
        self.session_result_with_timings = Some(result_with_timings);

        for j in self.handler_threads.drain(..) {
            debug!("joining...: {:?}", j);
            assert_eq!(j.join().unwrap(), ());
        }
        debug!(
            "stop_threads(): successfully stopped threads by {:?}",
            std::thread::current()
        );
    }

    fn send_task(&self, task: TaskInQueue) {
        debug!("send_task()");
        self.schedulrable_transaction_sender
            .send(ChainedChannel::Payload(task))
            .unwrap();
    }

    fn end_session(&mut self) -> ResultWithTimings {
        debug!("end_session(): will end session...");
        if !self.is_active() {
            self.start_threads();
        }

        let next_sender_and_receiver = unbounded();
        let (_next_sender, next_receiver) = &next_sender_and_receiver;

        self.schedulrable_transaction_sender
            .send(ChainedChannel::new_channel(
                next_receiver.clone(),
                ControlFrame::EndSession,
            ))
            .unwrap();
        let res = self.result_receiver.recv().unwrap();

        (
            self.schedulrable_transaction_sender,
            self.schedulable_transaction_receiver,
        ) = next_sender_and_receiver;

        res
    }

    fn start_session(&mut self, context: SchedulingContext) {
        if self.is_active() {
            let next_sender_and_receiver = unbounded();
            let (_next_sender, next_receiver) = &next_sender_and_receiver;

            self.schedulrable_transaction_sender
                .send(ChainedChannel::new_channel(
                    next_receiver.clone(),
                    ControlFrame::StartSession(context.clone()),
                ))
                .unwrap();

            self.context = WeakSchedulingContext::new(context);
            (
                self.schedulrable_transaction_sender,
                self.schedulable_transaction_receiver,
            ) = next_sender_and_receiver;
        } else {
            self.context = WeakSchedulingContext::new(context);
            self.start_threads();
        }
    }
}

pub trait InstallableScheduler<SEA: ScheduleExecutionArg>: InstalledScheduler<SEA> {
    fn has_context(&self) -> bool;
    fn replace_context(&mut self, context: SchedulingContext);
}

pub trait SpawnableScheduler<TH: Handler<SEA>, SEA: ScheduleExecutionArg>:
    InstallableScheduler<SEA>
{
    fn spawn(
        pool: Arc<SchedulerPool<Self, TH, SEA>>,
        initial_context: SchedulingContext,
        handler: TH,
    ) -> Self
    where
        Self: Sized;

    fn should_retain_in_pool(&mut self) -> bool
    where
        Self: Sized;
}

impl<TH: Handler<SEA>, SEA: ScheduleExecutionArg> SpawnableScheduler<TH, SEA>
    for PooledScheduler<TH, SEA>
{
    fn spawn(
        pool: Arc<SchedulerPool<Self, TH, SEA>>,
        initial_context: SchedulingContext,
        handler: TH,
    ) -> Self {
        Self::do_spawn(pool, initial_context, handler)
    }

    fn should_retain_in_pool(&mut self) -> bool {
        const BITS_PER_HEX_DIGIT: usize = 4;
        let page_count = self.address_book.page_count();
        if page_count < 200_000 {
            info!(
                "[sch_{:0width$x}]: watchdog: address book size: {page_count}...",
                self.id(),
                width = SchedulerId::BITS as usize / BITS_PER_HEX_DIGIT,
            );
        } else {
            info!(
                "[sch_{:0width$x}]: watchdog: too big address book size: {page_count}...; retiring scheduler",
                self.id(),
                width = SchedulerId::BITS as usize / BITS_PER_HEX_DIGIT,
            );
            self.stop_thread_manager();
            return false;
        }

        let Some(pooled_duration) = self.pooled_since() else {
            return true;
        };
        if pooled_duration <= Duration::from_secs(600) {
            true
        } else {
            info!(
                "[sch_{:0width$x}]: watchdog: retiring unused scheduler...",
                self.id(),
                width = SchedulerId::BITS as usize / BITS_PER_HEX_DIGIT,
            );
            self.stop_thread_manager();
            false
        }
    }
}

enum TaskSource {
    Runnable,
    Retryable,
}

pub struct ScheduleStage;

impl ScheduleStage {
    fn attempt_lock_for_execution(
        unique_weight: &UniqueWeight,
        lock_attempts: &mut [LockAttempt],
    ) -> usize {
        // no short-cuircuit; we at least all need to add to the contended queue
        let mut lock_failure_count = 0;

        for attempt in lock_attempts.iter_mut() {
            Self::attempt_lock_address(unique_weight, attempt);

            match attempt.status {
                LockStatus::Succeded => {}
                LockStatus::Failed => {
                    lock_failure_count += 1;
                }
            }
        }

        lock_failure_count
    }

    fn attempt_lock_address(unique_weight: &UniqueWeight, attempt: &mut LockAttempt) {
        let mut page = attempt.target_page_mut();
        let tcuw = page.blocked_task_queue.heaviest_weight();

        let strictly_lockable = if tcuw.is_none() {
            true
        } else if tcuw.unwrap() == *unique_weight {
            true
        } else if attempt.requested_usage == RequestedUsage::Readonly
            && page
                .blocked_task_queue.heaviest_writing_task_weight()
                .map(|existing_unique_weight| unique_weight > *existing_unique_weight)
                .unwrap_or(true)
        {
            // this _read-only_ unique_weight is heavier than any of contened write locks.
            true
        } else {
            false
        };
        drop(page);

        if !strictly_lockable {
            attempt.status = LockStatus::Failed;
            return;
        }

        let LockAttempt {
            page,
            requested_usage,
            status,
            ..
        } = attempt;
        let mut page = page.as_mut();

        match page.current_usage {
            Usage::Unused => {
                page.current_usage = Usage::renew(*requested_usage);
                *status = LockStatus::Succeded;
            }
            Usage::Readonly(ref mut count) => match requested_usage {
                RequestedUsage::Readonly => {
                    *count += 1;
                    *status = LockStatus::Succeded;
                }
                RequestedUsage::Writable => {
                    *status = LockStatus::Failed;
                }
            },
            Usage::Writable => {
                *status = LockStatus::Failed;
            }
        }
    }

    fn reset_lock(attempt: &LockAttempt) -> bool {
        match attempt.status {
            LockStatus::Succeded => Self::unlock(attempt),
            LockStatus::Failed => {
                false // do nothing
            }
        }
    }

    fn unlock(attempt: &LockAttempt) -> bool {
        let mut is_unused_now = false;

        let mut page = attempt.target_page_mut();

        match &mut page.current_usage {
            Usage::Readonly(ref mut count) => match &attempt.requested_usage {
                RequestedUsage::Readonly => {
                    if *count == SOLE_USE_COUNT {
                        is_unused_now = true;
                    } else {
                        *count -= 1;
                    }
                }
                RequestedUsage::Writable => unreachable!(),
            },
            Usage::Writable => match &attempt.requested_usage {
                RequestedUsage::Writable => {
                    is_unused_now = true;
                }
                RequestedUsage::Readonly => unreachable!(),
            },
            Usage::Unused => unreachable!(),
        }

        if is_unused_now {
            page.current_usage = Usage::Unused;
        }

        is_unused_now
    }

    fn try_lock_for_task(
        (task_source, next_task): (TaskSource, TaskInQueue),
        retryable_task_queue: &mut WeightedTaskQueue,
    ) -> Option<TaskInQueue> {
        let from_runnable = matches!(task_source, TaskSource::Runnable);

        let lock_failure_count = Self::attempt_lock_for_execution(
            &next_task.unique_weight,
            &mut next_task.lock_attempts_mut(),
        );

        if lock_failure_count > 0 {
            Self::reset_lock_for_failed_execution(
                &next_task.unique_weight,
                &mut next_task.lock_attempts_mut(),
            );
            next_task
                .contention_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            if from_runnable {
                next_task.mark_as_contended();
                Task::index_with_pages(&next_task);
            }

            return None;
        }

        trace!(
            "successful lock: (from_runnable: {}) after {} contentions",
            from_runnable,
            next_task
                .contention_count
                .load(std::sync::atomic::Ordering::SeqCst)
        );

        if !from_runnable {
            // as soon as next tack is succeeded in locking, trigger re-checks on read only
            // addresses so that more readonly transactions can be executed
            next_task.mark_as_uncontended();

            for read_only_lock_attempt in next_task
                .lock_attempts_mut()
                .iter()
                .filter(|l| l.requested_usage == RequestedUsage::Readonly)
            {
                if let Some(heaviest_blocked_task) = read_only_lock_attempt
                    .target_page_mut()
                    .blocked_task_queue
                    .reindex(false, &next_task.unique_weight)
                {
                    assert!(heaviest_blocked_task.currently_contended());
                    retryable_task_queue
                        .entry(heaviest_blocked_task.unique_weight)
                        .or_insert(heaviest_blocked_task);
                }
            }
        }

        Some(next_task)
    }

    fn reset_lock_for_failed_execution(
        unique_weight: &UniqueWeight,
        lock_attempts: &[LockAttempt],
    ) {
        for l in lock_attempts {
            Self::reset_lock(l);
        }
    }

    fn unlock_after_execution(
        should_remove: bool,
        uq: &UniqueWeight,
        retryable_task_queue: &mut WeightedTaskQueue,
        lock_attempts: &[LockAttempt],
    ) {
        for unlock_attempt in lock_attempts {
            // skip reindex() bookkeeping unless contended?
            let heaviest_uncontended = unlock_attempt
                .target_page_mut()
                .blocked_task_queue
                .reindex(should_remove, uq);

            let is_unused_now = Self::reset_lock(unlock_attempt);
            if !is_unused_now {
                continue;
            }

            if let Some(uncontended_task) = heaviest_uncontended {
                assert!(uncontended_task.currently_contended());
                retryable_task_queue
                    .entry(uncontended_task.unique_weight)
                    .or_insert(uncontended_task);
            }
        }
    }
}

impl<TH, SEA> InstalledScheduler<SEA> for PooledScheduler<TH, SEA>
where
    TH: Handler<SEA>,
    SEA: ScheduleExecutionArg,
{
    fn id(&self) -> SchedulerId {
        self.thread_manager.read().unwrap().scheduler_id
    }

    fn context(&self) -> SchedulingContext {
        self.thread_manager
            .read()
            .unwrap()
            .active_context()
            .unwrap()
    }

    fn schedule_execution(&self, transaction_with_index: SEA::TransactionWithIndex<'_>) {
        transaction_with_index.with_transaction_and_index(|transaction, index| {
            let locks = transaction.get_account_locks_unchecked();
            let writable_lock_iter = locks.writable.iter().map(|address| {
                LockAttempt::new(self.address_book.load(**address), RequestedUsage::Writable)
            });
            let readonly_lock_iter = locks.readonly.iter().map(|address| {
                LockAttempt::new(self.address_book.load(**address), RequestedUsage::Readonly)
            });
            let locks = writable_lock_iter
                .chain(readonly_lock_iter)
                .collect::<Vec<_>>();
            let uw = UniqueWeight::max_value() - index as UniqueWeight;
            let task = Task::new_for_queue(uw, transaction.clone(), locks);
            self.ensure_thread_manager_started().send_task(task);
        });
    }

    fn wait_for_termination(&mut self, wait_reason: &WaitReason) -> Option<ResultWithTimings> {
        if self.completed_result_with_timings.is_none() {
            self.completed_result_with_timings =
                Some(self.thread_manager.write().unwrap().end_session());
        }

        if wait_reason.is_paused() {
            None
        } else {
            self.completed_result_with_timings.take()
        }
    }

    fn return_to_pool(mut self: Box<Self>) {
        let pool = self.thread_manager.read().unwrap().pool.clone();
        self.pooled_now();
        pool.return_scheduler(self);
    }
}

#[derive(Default)]
struct SchedulingStateMachine {
    retryable_task_queue: WeightedTaskQueue,
    active_task_count: usize,
    handled_task_count: usize,
    reschedule_count: usize,
    rescheduled_task_count: usize,
    total_task_count: usize,
}

impl SchedulingStateMachine {
    fn is_empty(&self) -> bool {
        self.active_task_count == 0
    }

    fn retryable_task_count(&self) -> usize {
        self.retryable_task_queue.len()
    }

    fn active_task_count(&self) -> usize {
        self.active_task_count
    }

    fn handled_task_count(&self) -> usize {
        self.handled_task_count
    }

    fn reschedule_count(&self) -> usize {
        self.reschedule_count
    }

    fn rescheduled_task_count(&self) -> usize {
        self.rescheduled_task_count
    }

    fn total_task_count(&self) -> usize {
        self.total_task_count
    }

    fn schedule_new_task(&mut self, task: TaskInQueue) -> Option<TaskInQueue> {
        self.total_task_count += 1;
        self.active_task_count += 1;
        ScheduleStage::try_lock_for_task(
            (TaskSource::Runnable, task),
            &mut self.retryable_task_queue,
        )
    }

    fn schedule_retryable_task(&mut self) -> Option<TaskInQueue> {
        self.retryable_task_queue
            .pop_last()
            .and_then(|(_, task)| {
                self.reschedule_count += 1;
                ScheduleStage::try_lock_for_task(
                    (TaskSource::Retryable, task),
                    &mut self.retryable_task_queue,
                )
            })
            .map(|task| {
                self.rescheduled_task_count += 1;
                task
            })
    }

    fn deschedule_task(&mut self, ee: &Box<ExecutionEnvironment>) {
        self.active_task_count -= 1;
        self.handled_task_count += 1;
        let should_remove = ee
            .task
            .contention_count
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0;
        ScheduleStage::unlock_after_execution(
            should_remove,
            &ee.task.unique_weight,
            &mut self.retryable_task_queue,
            &mut ee.task.lock_attempts_mut(),
        );
    }
}

impl<TH, SEA> InstallableScheduler<SEA> for PooledScheduler<TH, SEA>
where
    TH: Handler<SEA>,
    SEA: ScheduleExecutionArg,
{
    fn has_context(&self) -> bool {
        true // consider to remove this method entirely???
    }

    fn replace_context(&mut self, context: SchedulingContext) {
        self.thread_manager.write().unwrap().start_session(context);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        assert_matches::assert_matches,
        solana_runtime::{
            bank::Bank,
            bank_forks::BankForks,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            installed_scheduler_pool::{BankWithScheduler, SchedulingContext},
            prioritization_fee_cache::PrioritizationFeeCache,
        },
        solana_sdk::{
            clock::MAX_PROCESSING_AGE,
            pubkey::Pubkey,
            signer::keypair::Keypair,
            system_transaction,
            transaction::{SanitizedTransaction, TransactionError},
        },
        std::{sync::Arc, thread::JoinHandle},
    };

    #[test]
    fn test_scheduler_pool_new() {
        solana_logger::setup();

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);

        // this indirectly proves that there should be circular link because there's only one Arc
        // at this moment now
        assert_eq!((Arc::strong_count(&pool), Arc::weak_count(&pool)), (1, 1));
        let debug = format!("{pool:#?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_scheduler_spawn() {
        solana_logger::setup();

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank);
        let scheduler = pool.take_scheduler(context);

        let debug = format!("{scheduler:#?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_scheduler_pool_filo() {
        solana_logger::setup();

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = DefaultSchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(SchedulingMode::BlockVerification, bank);

        let mut scheduler1 = pool.do_take_scheduler(context.clone());
        let scheduler_id1 = scheduler1.id();
        let mut scheduler2 = pool.do_take_scheduler(context.clone());
        let scheduler_id2 = scheduler2.id();
        assert_ne!(scheduler_id1, scheduler_id2);

        assert_matches!(
            scheduler1.wait_for_termination(&WaitReason::TerminatedToFreeze),
            None
        );
        pool.return_scheduler(scheduler1);
        assert_matches!(
            scheduler2.wait_for_termination(&WaitReason::TerminatedToFreeze),
            None
        );
        pool.return_scheduler(scheduler2);

        let scheduler3 = pool.do_take_scheduler(context.clone());
        assert_eq!(scheduler_id2, scheduler3.id());
        let scheduler4 = pool.do_take_scheduler(context.clone());
        assert_eq!(scheduler_id1, scheduler4.id());
    }

    #[test]
    fn test_scheduler_pool_context_drop_unless_reinitialized() {
        solana_logger::setup();

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = DefaultSchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(SchedulingMode::BlockVerification, bank);

        let mut scheduler = pool.do_take_scheduler(context.clone());

        assert!(scheduler.has_context());
        assert_matches!(
            scheduler.wait_for_termination(&WaitReason::PausedForRecentBlockhash),
            None
        );
        assert!(scheduler.has_context());
        assert_matches!(
            scheduler.wait_for_termination(&WaitReason::TerminatedToFreeze),
            None
        );
        assert!(!scheduler.has_context());
    }

    #[test]
    fn test_scheduler_pool_context_replace() {
        solana_logger::setup();

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = DefaultSchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
        let old_bank = &Arc::new(Bank::default_for_tests());
        let new_bank = &Arc::new(Bank::default_for_tests());
        assert!(!Arc::ptr_eq(old_bank, new_bank));

        let old_context =
            &SchedulingContext::new(SchedulingMode::BlockVerification, old_bank.clone());
        let new_context =
            &SchedulingContext::new(SchedulingMode::BlockVerification, new_bank.clone());

        let mut scheduler = pool.do_take_scheduler(old_context.clone());
        let scheduler_id = scheduler.id();
        assert_matches!(
            scheduler.wait_for_termination(&WaitReason::TerminatedToFreeze),
            None
        );
        pool.return_scheduler(scheduler);

        let scheduler = pool.take_scheduler(new_context.clone());
        assert_eq!(scheduler_id, scheduler.id());
        assert!(Arc::ptr_eq(scheduler.context().bank(), new_bank));
    }

    #[test]
    fn test_scheduler_pool_install_into_bank_forks() {
        solana_logger::setup();

        let bank = Bank::default_for_tests();
        let bank_forks = BankForks::new_rw_arc(bank);
        let mut bank_forks = bank_forks.write().unwrap();
        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);
        bank_forks.install_scheduler_pool(pool);
    }

    #[test]
    fn test_scheduler_install_into_bank() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let child_bank = Bank::new_from_parent(bank, &Pubkey::default(), 1);

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);

        let bank = Bank::default_for_tests();
        let bank_forks = BankForks::new_rw_arc(bank);
        let mut bank_forks = bank_forks.write().unwrap();

        // existing banks in bank_forks shouldn't process transactions anymore in general, so
        // shouldn't be touched
        assert!(!bank_forks
            .working_bank_with_scheduler()
            .has_installed_scheduler());
        bank_forks.install_scheduler_pool(pool);
        assert!(!bank_forks
            .working_bank_with_scheduler()
            .has_installed_scheduler());

        let mut child_bank = bank_forks.insert(child_bank);
        assert!(child_bank.has_installed_scheduler());
        bank_forks.remove(child_bank.slot());
        child_bank.drop_scheduler();
        assert!(!child_bank.has_installed_scheduler());
    }

    #[test]
    fn test_scheduler_schedule_execution_success() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let tx0 = &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &solana_sdk::pubkey::new_rand(),
            2,
            genesis_config.hash(),
        ));
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

        assert_eq!(bank.transaction_count(), 0);
        let scheduler = pool.take_scheduler(context);
        scheduler.schedule_execution(&(tx0, 0));
        let bank = BankWithScheduler::new(bank, Some(scheduler));
        assert_matches!(bank.wait_for_completed_scheduler(), Some((Ok(()), _)));
        assert_eq!(bank.transaction_count(), 1);
    }

    #[test]
    fn test_scheduler_schedule_execution_failure() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let unfunded_keypair = Keypair::new();
        let tx0 = &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &unfunded_keypair,
            &solana_sdk::pubkey::new_rand(),
            2,
            genesis_config.hash(),
        ));
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

        assert_eq!(bank.transaction_count(), 0);
        let scheduler = pool.take_scheduler(context);
        scheduler.schedule_execution(&(tx0, 0));
        assert_eq!(bank.transaction_count(), 0);

        let tx1 = &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &solana_sdk::pubkey::new_rand(),
            3,
            genesis_config.hash(),
        ));
        assert_matches!(
            bank.simulate_transaction_unchecked(tx1.clone()).result,
            Ok(_)
        );
        scheduler.schedule_execution(&(tx1, 0));
        // transaction_count should remain same as scheduler should be bailing out.
        assert_eq!(bank.transaction_count(), 0);

        let bank = BankWithScheduler::new(bank, Some(scheduler));
        assert_matches!(
            bank.wait_for_completed_scheduler(),
            Some((
                Err(solana_sdk::transaction::TransactionError::AccountNotFound),
                _timings
            ))
        );
    }

    #[derive(Debug)]
    struct AsyncScheduler<const TRIGGER_RACE_CONDITION: bool>(
        PooledScheduler<DefaultTransactionHandler, DefaultScheduleExecutionArg>,
        Mutex<Vec<JoinHandle<ResultWithTimings>>>,
    );

    impl<const TRIGGER_RACE_CONDITION: bool> InstalledScheduler<DefaultScheduleExecutionArg>
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        fn id(&self) -> SchedulerId {
            self.0.id()
        }

        fn context(&self) -> SchedulingContext {
            self.0.context().clone()
        }

        fn schedule_execution<'a>(
            &'a self,
            &(transaction, index): <DefaultScheduleExecutionArg as ScheduleExecutionArg>::TransactionWithIndex<'a>,
        ) {
            todo!();
            /*
            let transaction_and_index = (transaction.clone(), index);
            let context = self.context().clone();
            let pool = self.0.pool.clone();

            self.1.lock().unwrap().push(std::thread::spawn(move || {
                // intentionally sleep to simulate race condition where register_recent_blockhash
                // is run before finishing executing scheduled transactions
                std::thread::sleep(std::time::Duration::from_secs(1));

                let mut result = Ok(());
                let mut timings = ExecuteTimings::default();

                <DefaultTransactionHandler as Handler<DefaultScheduleExecutionArg>>::handle(
                    &DefaultTransactionHandler,
                    &mut result,
                    &mut timings,
                    context.bank(),
                    &transaction_and_index.0,
                    transaction_and_index.1,
                    &pool,
                );
                (result, timings)
            }));
            */
        }

        fn wait_for_termination(&mut self, reason: &WaitReason) -> Option<ResultWithTimings> {
            todo!();
            /*
            if TRIGGER_RACE_CONDITION && matches!(reason, WaitReason::PausedForRecentBlockhash) {
                // this is equivalent to NOT calling wait_for_paused_scheduler() in
                // register_recent_blockhash().
                return None;
            }

            let mut overall_result = Ok(());
            let mut overall_timings = ExecuteTimings::default();
            for handle in self.1.lock().unwrap().drain(..) {
                let (result, timings) = handle.join().unwrap();
                match result {
                    Ok(()) => {}
                    Err(e) => overall_result = Err(e),
                }
                overall_timings.accumulate(&timings);
            }
            *self.0.result_with_timings.lock().unwrap() = Some((overall_result, overall_timings));

            self.0.wait_for_termination(reason)
            */
        }

        fn return_to_pool(self: Box<Self>) {
            Box::new(self.0).return_to_pool()
        }
    }

    impl<const TRIGGER_RACE_CONDITION: bool>
        SpawnableScheduler<DefaultTransactionHandler, DefaultScheduleExecutionArg>
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        fn spawn(
            pool: Arc<SchedulerPool<Self, DefaultTransactionHandler, DefaultScheduleExecutionArg>>,
            initial_context: SchedulingContext,
            handler: DefaultTransactionHandler,
        ) -> Self {
            todo!();
            /*
            AsyncScheduler::<TRIGGER_RACE_CONDITION>(
                PooledScheduler::<DefaultTransactionHandler, DefaultScheduleExecutionArg> {
                    id: thread_rng().gen::<SchedulerId>(),
                    pool: SchedulerPool::new(
                        pool.log_messages_bytes_limit,
                        pool.transaction_status_sender.clone(),
                        pool.replay_vote_sender.clone(),
                        pool.prioritization_fee_cache.clone(),
                    ),
                    context: Some(initial_context),
                    result_with_timings: Mutex::default(),
                    handler,
                    _phantom: PhantomData,
                },
                Mutex::new(vec![]),
            )
            */
        }

        fn should_retain_in_pool(&mut self) -> bool {
            todo!();
        }
    }

    impl<const TRIGGER_RACE_CONDITION: bool> InstallableScheduler<DefaultScheduleExecutionArg>
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        fn has_context(&self) -> bool {
            self.0.has_context()
        }

        fn replace_context(&mut self, context: SchedulingContext) {
            self.0.replace_context(context)
        }
    }

    fn do_test_scheduler_schedule_execution_recent_blockhash_edge_case<
        const TRIGGER_RACE_CONDITION: bool,
    >() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let very_old_valid_tx =
            SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                &mint_keypair,
                &solana_sdk::pubkey::new_rand(),
                2,
                genesis_config.hash(),
            ));
        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        for _ in 0..MAX_PROCESSING_AGE {
            bank.fill_bank_with_ticks_for_tests();
            bank.freeze();
            bank = Arc::new(Bank::new_from_parent(
                bank.clone(),
                &Pubkey::default(),
                bank.slot().checked_add(1).unwrap(),
            ));
        }
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::<
            AsyncScheduler<TRIGGER_RACE_CONDITION>,
            DefaultTransactionHandler,
            DefaultScheduleExecutionArg,
        >::new_dyn(None, None, None, ignored_prioritization_fee_cache);
        let scheduler = pool.take_scheduler(context);

        let bank = BankWithScheduler::new(bank, Some(scheduler));
        assert_eq!(bank.transaction_count(), 0);

        // schedule but not immediately execute transaction
        bank.schedule_transaction_executions([(&very_old_valid_tx, &0)].into_iter());
        // this calls register_recent_blockhash internally
        bank.fill_bank_with_ticks_for_tests();

        if TRIGGER_RACE_CONDITION {
            // very_old_valid_tx is wrongly handled as expired!
            assert_matches!(
                bank.wait_for_completed_scheduler(),
                Some((Err(TransactionError::BlockhashNotFound), _))
            );
            assert_eq!(bank.transaction_count(), 0);
        } else {
            assert_matches!(bank.wait_for_completed_scheduler(), Some((Ok(()), _)));
            assert_eq!(bank.transaction_count(), 1);
        }
    }

    #[test]
    fn test_scheduler_schedule_execution_recent_blockhash_edge_case_with_race() {
        do_test_scheduler_schedule_execution_recent_blockhash_edge_case::<true>();
    }

    #[test]
    fn test_scheduler_schedule_execution_recent_blockhash_edge_case_without_race() {
        do_test_scheduler_schedule_execution_recent_blockhash_edge_case::<false>();
    }
}
