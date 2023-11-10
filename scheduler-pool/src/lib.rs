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
        transaction::{Result, SanitizedTransaction},
    },
    solana_vote::vote_sender_types::ReplayVoteSender,
    std::{
        fmt::Debug,
        marker::PhantomData,
        sync::{atomic::AtomicUsize, Arc, Mutex, RwLock, Weak},
        thread::JoinHandle,
    },
};
use std::sync::RwLockReadGuard;

type UniqueWeight = u128;
type CU = u64;

type TaskIds = BTreeMapTaskIds;
#[derive(Debug, Default)]
pub struct BTreeMapTaskIds {
    task_ids: std::collections::BTreeMap<UniqueWeight, TaskInQueue>,
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
    _phantom: PhantomData<(T, TH, SEA)>,
}

pub type DefaultSchedulerPool = SchedulerPool<
    PooledScheduler<DefaultTransactionHandler, DefaultScheduleExecutionArg>,
    DefaultTransactionHandler,
    DefaultScheduleExecutionArg,
>;

impl<T: SpawnableScheduler<TH, SEA>, TH: Handler<SEA>, SEA: ScheduleExecutionArg>
    SchedulerPool<T, TH, SEA>
{
    pub fn new(
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| Self {
            schedulers: Mutex::default(),
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
            weak_self: weak_self.clone(),
            _phantom: PhantomData,
        })
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

    pub fn return_scheduler(&self, scheduler: Box<T>) {
        assert!(!scheduler.has_context());

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
}

impl<T: SpawnableScheduler<TH, SEA>, TH: Handler<SEA>, SEA: ScheduleExecutionArg>
    InstalledSchedulerPool<SEA> for SchedulerPool<T, TH, SEA>
{
    fn take_scheduler(&self, context: SchedulingContext) -> Box<dyn InstalledScheduler<SEA>> {
        self.do_take_scheduler(context)
    }
}

pub trait Handler<SEA: ScheduleExecutionArg>: Send + Sync + Debug + Sized + 'static {
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

#[derive(Debug)]
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
pub struct LockAttemptsInCell(std::cell::RefCell<Vec<LockAttempt>>);

impl LockAttemptsInCell {
    fn new(ll: std::cell::RefCell<Vec<LockAttempt>>) -> Self {
        Self(ll)
    }
}

#[derive(Debug)]
pub struct Task {
    unique_weight: UniqueWeight,
    pub tx: (SanitizedTransaction, LockAttemptsInCell), // actually should be Bundle
    pub contention_count: std::sync::atomic::AtomicUsize,
    pub uncontended: std::sync::atomic::AtomicUsize,
}

impl Task {
    pub fn new_for_queue(
        unique_weight: UniqueWeight,
        tx: (SanitizedTransaction, Vec<LockAttempt>),
    ) -> TaskInQueue {
        TaskInQueue::new(Self {
            unique_weight,
            tx: (tx.0, LockAttemptsInCell::new(std::cell::RefCell::new(tx.1))),
            uncontended: Default::default(),
            contention_count: Default::default(),
        })
    }
    #[inline(never)]
    pub fn clone_in_queue(this: &TaskInQueue) -> TaskInQueue {
        TaskInQueue::clone(this)
    }

    #[inline(never)]
    fn index_with_address_book(this: &TaskInQueue) {
        for lock_attempt in &*this.lock_attempts_mut() {
            lock_attempt
                .target_page_mut()
                .task_ids
                .insert_task(this.unique_weight, Task::clone_in_queue(this));

            if lock_attempt.requested_usage == RequestedUsage::Writable {
                lock_attempt
                    .target_page_mut()
                    .write_task_ids
                    .insert(this.unique_weight);
            }
        }
    }

    fn lock_attempts_mut(&self) -> std::cell::RefMut<'_, Vec<LockAttempt>> {
        self.tx.1 .0.borrow_mut()
    }

    pub fn currently_contended(&self) -> bool {
        self.uncontended.load(std::sync::atomic::Ordering::SeqCst) == 1
    }

    pub fn already_finished(&self) -> bool {
        self.uncontended.load(std::sync::atomic::Ordering::SeqCst) == 3
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

    fn mark_as_finished(&self) {
        assert!(!self.already_finished() && !self.currently_contended());
        self.uncontended
            .store(3, std::sync::atomic::Ordering::SeqCst)
    }
}

#[derive(Debug)]
pub struct LockAttempt {
    target: PageRc,
    status: LockStatus,
    requested_usage: RequestedUsage,
    pub heaviest_uncontended: Option<TaskInQueue>,
}

impl PageRc {
    fn page_mut(&self) -> std::cell::RefMut<'_, Page> {
        self.0 .0 .0.borrow_mut()
    }
}

impl LockAttempt {
    pub fn new(target: PageRc, requested_usage: RequestedUsage) -> Self {
        Self {
            target,
            status: LockStatus::Succeded,
            requested_usage,
            heaviest_uncontended: Default::default(),
        }
    }

    pub fn clone_for_test(&self) -> Self {
        Self {
            target: self.target.clone(),
            status: LockStatus::Succeded,
            requested_usage: self.requested_usage,
            heaviest_uncontended: Default::default(),
        }
    }

    fn target_page_mut(&self) -> std::cell::RefMut<'_, Page> {
        self.target.page_mut()
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
    address_str: String,
    current_usage: Usage,
    next_usage: Usage,
    task_ids: TaskIds,
    write_task_ids: std::collections::BTreeSet<UniqueWeight>,
}

impl Page {
    fn new(address: &Pubkey, current_usage: Usage) -> Self {
        Self {
            address_str: format!("{}", address),
            current_usage,
            next_usage: Usage::Unused,
            task_ids: Default::default(),
            write_task_ids: Default::default(),
        }
    }

    fn switch_to_next_usage(&mut self) {
        self.current_usage = self.next_usage;
        self.next_usage = Usage::Unused;
    }
}

impl BTreeMapTaskIds {
    #[inline(never)]
    pub fn insert_task(&mut self, u: UniqueWeight, task: TaskInQueue) {
        let pre_existed = self.task_ids.insert(u, task);
        assert!(pre_existed.is_none()); //, "identical shouldn't exist: {:?}", unique_weight);
    }

    #[inline(never)]
    pub fn remove_task(&mut self, u: &UniqueWeight) {
        let removed_entry = self.task_ids.remove(u);
        assert!(removed_entry.is_some());
    }

    #[inline(never)]
    fn heaviest_task_cursor(&self) -> impl Iterator<Item = &TaskInQueue> {
        self.task_ids.values().rev()
    }

    pub fn heaviest_task_id(&mut self) -> Option<UniqueWeight> {
        self.task_ids.last_entry().map(|j| *j.key())
    }

    #[inline(never)]
    fn reindex(&mut self, should_remove: bool, uq: &UniqueWeight) -> Option<TaskInQueue> {
        if should_remove {
            self.remove_task(uq);
        }

        self.heaviest_task_cursor()
            .find(|task| {
                assert!(!task.already_finished());
                task.currently_contended()
            })
            .map(|task| Task::clone_in_queue(task))
    }
}

type PageRcInner = Arc<(
    std::cell::RefCell<Page>,
    //SkipListTaskIds,
    std::sync::atomic::AtomicUsize,
)>;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct PageRc(by_address::ByAddress<PageRcInner>);
unsafe impl Send for PageRc {}
unsafe impl Sync for PageRc {}
unsafe impl Send for LockAttemptsInCell {}
unsafe impl Sync for LockAttemptsInCell {}
type WeightedTaskIds2 =
    std::collections::BTreeMap<UniqueWeight, (TaskInQueue, std::collections::HashSet<PageRc>)>;

type AddressMap = std::sync::Arc<dashmap::DashMap<Pubkey, PageRc>>;
#[derive(Default, Debug)]
pub struct AddressBook {
    book: AddressMap,
    uncontended_task_ids: WeightedTaskIds2,
}

impl AddressBook {
    #[inline(never)]
    fn attempt_lock_address(
        from_runnable: bool,
        unique_weight: &UniqueWeight,
        attempt: &mut LockAttempt,
    ) {
        let tcuw = attempt
            //.target_contended_unique_weights()
            .target_page_mut()
            .task_ids
            .heaviest_task_id();

        let strictly_lockable = if tcuw.is_none() {
            true
        } else if tcuw.unwrap() == *unique_weight {
            true
        } else if attempt.requested_usage == RequestedUsage::Readonly
            && attempt
                .target_page_mut()
                .write_task_ids
                .last()
                .map(|j| unique_weight > j)
                .unwrap_or(true)
        {
            true
        } else {
            false
        };

        if !strictly_lockable {
            attempt.status = LockStatus::Failed;
            return;
        }

        let LockAttempt {
            target,
            requested_usage,
            status, /*, remembered*/
            ..
        } = attempt;
        let mut page = target.page_mut();

        let next_usage = page.next_usage;
        match page.current_usage {
            Usage::Unused => {
                assert_eq!(page.next_usage, Usage::Unused);
                page.current_usage = Usage::renew(*requested_usage);
                *status = LockStatus::Succeded;
            }
            Usage::Readonly(ref mut count) => match requested_usage {
                RequestedUsage::Readonly => {
                    // prevent newer read-locks (even from runnable too)
                    match next_usage {
                        Usage::Unused => {
                            *count += 1;
                            *status = LockStatus::Succeded;
                        }
                        Usage::Readonly(_) | Usage::Writable => {
                            *status = LockStatus::Failed;
                        }
                    }
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

    fn reset_lock(&mut self, attempt: &mut LockAttempt, after_execution: bool) -> bool {
        match attempt.status {
            LockStatus::Succeded => self.unlock(attempt),
            LockStatus::Failed => {
                false // do nothing
            }
        }
    }

    #[inline(never)]
    fn unlock(&mut self, attempt: &mut LockAttempt) -> bool {
        //debug_assert!(attempt.is_success());

        let mut newly_uncontended = false;

        let mut page = attempt.target_page_mut();

        match &mut page.current_usage {
            Usage::Readonly(ref mut count) => match &attempt.requested_usage {
                RequestedUsage::Readonly => {
                    if *count == SOLE_USE_COUNT {
                        newly_uncontended = true;
                    } else {
                        *count -= 1;
                    }
                }
                RequestedUsage::Writable => unreachable!(),
            },
            Usage::Writable => match &attempt.requested_usage {
                RequestedUsage::Writable => {
                    newly_uncontended = true;
                }
                RequestedUsage::Readonly => unreachable!(),
            },
            Usage::Unused => unreachable!(),
        }

        if newly_uncontended {
            page.current_usage = Usage::Unused;
        }

        newly_uncontended
    }

    #[inline(never)]
    fn cancel(&mut self, attempt: &mut LockAttempt) {
        let mut page = attempt.target_page_mut();

        match page.next_usage {
            Usage::Unused => {
                unreachable!();
            }
            // support multiple readonly locks!
            Usage::Readonly(_) | Usage::Writable => {
                page.next_usage = Usage::Unused;
            }
        }
    }

    pub fn preloader(&self) -> Preloader {
        Preloader {
            book: std::sync::Arc::clone(&self.book),
        }
    }
}

#[derive(Debug)]
pub struct Preloader {
    book: AddressMap,
}

impl Preloader {
    #[inline(never)]
    pub fn load(&self, address: Pubkey) -> PageRc {
        PageRc::clone(&self.book.entry(address).or_insert_with(|| {
            PageRc(by_address::ByAddress(PageRcInner::new((
                core::cell::RefCell::new(Page::new(&address, Usage::unused())),
                //Default::default(),
                AtomicUsize::default(),
            ))))
        }))
    }
}

type TaskQueueEntry<'a> = std::collections::btree_map::Entry<'a, UniqueWeight, TaskInQueue>;
type TaskQueueOccupiedEntry<'a> =
    std::collections::btree_map::OccupiedEntry<'a, UniqueWeight, TaskInQueue>;

use enum_dispatch::enum_dispatch;

#[enum_dispatch]
enum ModeSpecificTaskQueue {
    BlockVerification(ChannelBackedTaskQueue),
}

#[enum_dispatch(ModeSpecificTaskQueue)]
trait TaskQueueReader {
    fn add_to_schedule(&mut self, unique_weight: UniqueWeight, task: TaskInQueue);
    fn heaviest_entry_to_execute(&mut self) -> Option<TaskInQueue>;
    fn task_count_hint(&self) -> usize;
    fn has_no_task_hint(&self) -> bool;
    fn take_buffered_flush(&mut self) -> Option<Flushable<TaskInQueue>>;
    fn is_backed_by_channel(&self) -> bool;
}

impl TaskQueueReader for TaskQueue {
    #[inline(never)]
    fn add_to_schedule(&mut self, unique_weight: UniqueWeight, task: TaskInQueue) {
        //trace!("TaskQueue::add(): {:?}", unique_weight);
        let pre_existed = self.tasks.insert(unique_weight, task);
        assert!(pre_existed.is_none()); //, "identical shouldn't exist: {:?}", unique_weight);
    }

    #[inline(never)]
    fn heaviest_entry_to_execute(&mut self) -> Option<TaskInQueue> {
        self.tasks.pop_last().map(|(_k, v)| v)
    }

    fn task_count_hint(&self) -> usize {
        self.tasks.len()
    }

    fn has_no_task_hint(&self) -> bool {
        self.tasks.is_empty()
    }

    fn take_buffered_flush(&mut self) -> Option<Flushable<TaskInQueue>> {
        None
    }

    fn is_backed_by_channel(&self) -> bool {
        false
    }
}

#[derive(Default, Debug, Clone)]
pub struct TaskQueue {
    tasks: std::collections::BTreeMap<UniqueWeight, TaskInQueue>,
    //tasks: im::OrdMap<UniqueWeight, TaskInQueue>,
    //tasks: im::HashMap<UniqueWeight, TaskInQueue>,
    //tasks: std::sync::Arc<dashmap::DashMap<UniqueWeight, TaskInQueue>>,
}

struct ChannelBackedTaskQueue {
    channel: crossbeam_channel::Receiver<SchedulablePayload>,
    buffered_task: Option<TaskInQueue>,
    buffered_flush: bool,
}

impl ChannelBackedTaskQueue {
    fn new(channel: &crossbeam_channel::Receiver<SchedulablePayload>) -> Self {
        Self {
            channel: channel.clone(),
            buffered_task: None,
            buffered_flush: false,
        }
    }

    fn buffer(&mut self, task: TaskInQueue) {
        assert!(self.buffered_task.is_none());
        self.buffered_task = Some(task);
    }
}

#[derive(Debug)]
pub struct ExecutionEnvironment {
    //accounts: Vec<i8>,
    pub unique_weight: UniqueWeight,
    pub task: TaskInQueue,
    pub finalized_lock_attempts: Vec<LockAttempt>,
    pub is_reindexed: bool,
    pub execution_result:
        Option<std::result::Result<(), solana_sdk::transaction::TransactionError>>,
    pub finish_time: Option<std::time::SystemTime>,
    pub thx: usize,
    pub execution_us: u64,
    pub execution_cpu_us: u128,
}

impl ExecutionEnvironment {
    #[inline(never)]
    fn reindex_with_address_book(&mut self) {
        assert!(!self.is_reindexed());
        self.is_reindexed = true;

        let uq = self.unique_weight;
        let should_remove = self
            .task
            .contention_count
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0;
        for lock_attempt in self.finalized_lock_attempts.iter_mut() {
            let ll = lock_attempt
                .target_page_mut()
                .task_ids
                .reindex(should_remove, &uq);
            if let Some(heaviest_uncontended) = ll {
                lock_attempt.heaviest_uncontended = Some(heaviest_uncontended);
            };

            if should_remove && lock_attempt.requested_usage == RequestedUsage::Writable {
                lock_attempt.target_page_mut().write_task_ids.remove(&uq);
            }
        }
    }

    fn is_reindexed(&self) -> bool {
        self.is_reindexed
    }

    pub fn is_aborted(&self) -> bool {
        if let Some(r) = &self.execution_result {
            r.is_err()
        } else {
            false
        }
    }
}

pub struct SchedulablePayload(pub Flushable<TaskInQueue>);
pub struct ExecutablePayload(pub Flushable<Box<ExecutionEnvironment>>);
pub struct UnlockablePayload<T>(pub Box<ExecutionEnvironment>, pub T);
pub struct ExaminablePayload<T>(pub Flushable<(Box<ExecutionEnvironment>, T)>);

pub enum Flushable<T> {
    Payload(T),
    Flush,
}

impl TaskQueueReader for ChannelBackedTaskQueue {
    #[inline(never)]
    fn add_to_schedule(&mut self, unique_weight: UniqueWeight, task: TaskInQueue) {
        self.buffer(task)
    }

    fn task_count_hint(&self) -> usize {
        self.channel.len()
            + (match self.buffered_task {
                None => 0,
                Some(_) => 1,
            })
    }

    fn has_no_task_hint(&self) -> bool {
        self.task_count_hint() == 0
    }

    fn take_buffered_flush(&mut self) -> Option<Flushable<TaskInQueue>> {
        std::mem::take(&mut self.buffered_flush).then_some(Flushable::Flush)
    }

    #[inline(never)]
    fn heaviest_entry_to_execute(&mut self) -> Option<TaskInQueue> {
        match self.buffered_task.take() {
            Some(task) => Some(task),
            None => {
                // unblocking recv must have been gurantted to succeed at the time of this method
                // invocation
                match self.channel.try_recv().unwrap() {
                    SchedulablePayload(Flushable::Payload(task)) => Some(task),
                    SchedulablePayload(Flushable::Flush) => {
                        assert!(!self.buffered_flush);
                        self.buffered_flush = true;
                        None
                    }
                }
            }
        }
    }

    fn is_backed_by_channel(&self) -> bool {
        true
    }
}

// Currently, simplest possible implementation (i.e. single-threaded)
// this will be replaced with more proper implementation...
// not usable at all, especially for mainnet-beta
#[derive(Debug)]
pub struct PooledScheduler<TH: Handler<SEA>, SEA: ScheduleExecutionArg> {
    id: SchedulerId,
    pool: Arc<SchedulerPool<Self, TH, SEA>>,
    context: Option<SchedulingContext>, // to be removed
    result_with_timings: Mutex<Option<ResultWithTimings>>, // to be removed
    handler: TH,
    address_book: Mutex<AddressBook>,
    preloader: Arc<Preloader>,
    thread_manager: RwLock<ThreadManager>,
    _phantom: PhantomData<SEA>,
}

#[derive(Default, Debug)]
struct ThreadManager {
    scheduler_thread: Option<JoinHandle<()>>,
    handler_threads: Vec<JoinHandle<()>>,
}

impl<TH: Handler<SEA>, SEA: ScheduleExecutionArg> PooledScheduler<TH, SEA> {
    pub fn do_spawn(
        pool: Arc<SchedulerPool<Self, TH, SEA>>,
        initial_context: SchedulingContext,
        handler: TH,
    ) -> Self {
        let address_book = AddressBook::default();
        let preloader = Arc::new(address_book.preloader());

        let mut new = Self {
            id: thread_rng().gen::<SchedulerId>(),
            pool,
            context: Some(initial_context),
            result_with_timings: Mutex::default(),
            handler,
            address_book: Mutex::new(address_book),
            preloader,
            thread_manager: RwLock::default(),
            _phantom: PhantomData,
        };
        drop(new.ensure_threads());
        new
    }

    #[must_use]
    fn ensure_threads(&self) -> RwLockReadGuard<'_, ThreadManager> {
        loop {
            let r = self.thread_manager.read().unwrap();
            if r.is_active() {
                return r;
            } else {
                drop(r);
                let mut w = self.thread_manager.write().unwrap();
                w.start_threads();
                drop(w);
            }
        }
    }
}

trait WithChannelPair: Send + Sync {
    fn unwrap_channel_pair(&mut self) -> usize;
}

enum SessionedChannel {
    Payload(usize),
    NextContext(SchedulingContext),
    NextSession(Box<dyn WithChannelPair>),
    Stop,
}

impl ThreadManager {
    fn is_active(&self) -> bool {
        self.scheduler_thread.is_some()
    }

    fn start_threads(&mut self) {
        let t = std::thread::Builder::new().name("aaaa").spawn(move || {
        }).unwrap();
        self.scheduler_thread = Some(t);
    }

    fn stop_threads(&self) {}
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
}

enum TaskSource {
    Runnable,
    Contended(std::collections::HashSet<PageRc>),
    Stuck,
}

enum TaskSelection {
    OnlyFromRunnable,
    OnlyFromContended(usize),
}

impl TaskSelection {
    fn should_proceed(&self) -> bool {
        match self {
            TaskSelection::OnlyFromRunnable => true,
            TaskSelection::OnlyFromContended(retry_count) => *retry_count > 0,
        }
    }

    fn runnable_exclusive(&self) -> bool {
        match self {
            TaskSelection::OnlyFromRunnable => true,
            TaskSelection::OnlyFromContended(_) => false,
        }
    }
}

#[inline(never)]
fn attempt_lock_for_execution<'a>(
    from_runnable: bool,
    address_book: &mut AddressBook,
    unique_weight: &UniqueWeight,
    lock_attempts: &mut [LockAttempt],
) -> (usize, usize) {
    // no short-cuircuit; we at least all need to add to the contended queue
    let mut unlockable_count = 0;
    let mut provisional_count = 0;

    for attempt in lock_attempts.iter_mut() {
        AddressBook::attempt_lock_address(from_runnable, unique_weight, attempt);

        match attempt.status {
            LockStatus::Succeded => {}
            LockStatus::Failed => {
                trace!(
                    "lock failed: {}/{:?}",
                    attempt.target_page_mut().address_str,
                    attempt.requested_usage
                );
                unlockable_count += 1;
            }
        }
    }

    (unlockable_count, provisional_count)
}

pub struct ScheduleStage {}
impl ScheduleStage {
    #[inline(never)]
    fn get_heaviest_from_contended<'a>(
        address_book: &'a mut AddressBook,
    ) -> Option<
        std::collections::btree_map::OccupiedEntry<
            'a,
            UniqueWeight,
            (TaskInQueue, std::collections::HashSet<PageRc>),
        >,
    > {
        address_book.uncontended_task_ids.last_entry()
    }

    #[inline(never)]
    fn select_next_task<'a>(
        runnable_queue: &'a mut ModeSpecificTaskQueue,
        address_book: &mut AddressBook,
        task_selection: &mut TaskSelection,
    ) -> Option<(TaskSource, TaskInQueue)> {
        let selected_heaviest_tasks = match task_selection {
            TaskSelection::OnlyFromRunnable => (runnable_queue.heaviest_entry_to_execute(), None),
            TaskSelection::OnlyFromContended(_) => {
                (None, Self::get_heaviest_from_contended(address_book))
            }
        };

        match selected_heaviest_tasks {
            (Some(heaviest_runnable_entry), None) => {
                trace!("select: runnable only");
                if task_selection.runnable_exclusive() {
                    let t = heaviest_runnable_entry; // .remove();
                    trace!("new task: {:032x}", t.unique_weight);
                    Some((TaskSource::Runnable, t))
                } else {
                    None
                }
            }
            (None, Some(weight_from_contended)) => {
                trace!("select: contended only");
                if task_selection.runnable_exclusive() {
                    None
                } else {
                    let t = weight_from_contended.remove();
                    Some((TaskSource::Contended(t.1), t.0))
                }
            }
            (Some(heaviest_runnable_entry), Some(weight_from_contended)) => {
                unreachable!("heaviest_entry_to_execute isn't idempotent....");
            }
            (None, None) => {
                trace!("select: none");
                None
            }
        }
    }

    #[inline(never)]
    fn pop_from_queue_then_lock(
        runnable_queue: &mut ModeSpecificTaskQueue,
        address_book: &mut AddressBook,
        task_selection: &mut TaskSelection,
        failed_lock_count: &mut usize,
    ) -> Option<(UniqueWeight, TaskInQueue, Vec<LockAttempt>)> {
        loop {
            if let Some((task_source, next_task)) =
                Self::select_next_task(runnable_queue, address_book, task_selection)
            {
                let from_runnable = matches!(task_source, TaskSource::Runnable);
                let unique_weight = next_task.unique_weight;

                let (unlockable_count, provisional_count) = attempt_lock_for_execution(
                    from_runnable,
                    address_book,
                    &unique_weight,
                    &mut next_task.lock_attempts_mut(),
                );

                if unlockable_count > 0 {
                    *failed_lock_count += 1;
                    if let TaskSelection::OnlyFromContended(ref mut retry_count) = task_selection {
                        *retry_count = retry_count.checked_sub(1).unwrap();
                    }

                    Self::reset_lock_for_failed_execution(
                        address_book,
                        &unique_weight,
                        &mut next_task.lock_attempts_mut(),
                    );
                    let lock_count = next_task.lock_attempts_mut().len();
                    next_task
                        .contention_count
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                    if from_runnable {
                        trace!(
                            "move to contended due to lock failure [{}/{}/{}]",
                            unlockable_count,
                            provisional_count,
                            lock_count
                        );
                        next_task.mark_as_contended();

                        Task::index_with_address_book(&next_task);

                        // maybe run lightweight prune logic on contended_queue here.
                    } else {
                        trace!(
                            "relock failed [{}/{}/{}]; remains in contended: {:?} contention: {}",
                            unlockable_count,
                            provisional_count,
                            lock_count,
                            &unique_weight,
                            next_task
                                .contention_count
                                .load(std::sync::atomic::Ordering::SeqCst)
                        );
                        //address_book.uncontended_task_ids.clear();
                    }

                    if from_runnable || matches!(task_source, TaskSource::Stuck) {
                        break;
                    } else if matches!(task_source, TaskSource::Contended(_)) {
                        break;
                    } else {
                        unreachable!();
                    }
                } else if provisional_count > 0 {
                    assert!(!from_runnable);
                    assert_eq!(unlockable_count, 0);
                    let lock_count = next_task.lock_attempts_mut().len();
                    trace!("provisional exec: [{}/{}]", provisional_count, lock_count);
                    next_task.mark_as_uncontended();
                    break;
                }

                trace!(
                    "successful lock: (from_runnable: {}) after {} contentions",
                    from_runnable,
                    next_task
                        .contention_count
                        .load(std::sync::atomic::Ordering::SeqCst)
                );

                assert!(!next_task.already_finished());
                if !from_runnable {
                    next_task.mark_as_uncontended();
                    if let TaskSource::Contended(uncontendeds) = task_source {
                        for lock_attempt in next_task.lock_attempts_mut().iter().filter(|l| l.requested_usage == RequestedUsage::Readonly /*&& uncontendeds.contains(&l.target)*/) {
                            if let Some(task) = lock_attempt.target_page_mut().task_ids.reindex(false, &unique_weight) {
                                if task.currently_contended() {
                                    let uti = address_book
                                        .uncontended_task_ids
                                        .entry(task.unique_weight).or_insert((task, Default::default()));
                                    uti.1.insert(lock_attempt.target.clone());
                                }
                            }
                        }
                    }
                }
                let lock_attempts = std::mem::take(&mut *next_task.lock_attempts_mut());

                return Some((unique_weight, next_task, lock_attempts));
            } else {
                break;
            }
        }

        None
    }

    #[inline(never)]
    fn reset_lock_for_failed_execution(
        address_book: &mut AddressBook,
        unique_weight: &UniqueWeight,
        lock_attempts: &mut [LockAttempt],
    ) {
        for l in lock_attempts {
            address_book.reset_lock(l, false);
        }
    }

    #[inline(never)]
    fn unlock_after_execution(address_book: &mut AddressBook, lock_attempts: &mut [LockAttempt]) {
        for mut l in lock_attempts {
            let newly_uncontended = address_book.reset_lock(&mut l, true);

            let mut page = l.target.page_mut();
            if newly_uncontended && page.next_usage == Usage::Unused {
                if let Some(task) = l.heaviest_uncontended.take() {
                    if task.currently_contended() {
                        let uti = address_book
                            .uncontended_task_ids
                            .entry(task.unique_weight)
                            .or_insert((task, Default::default()));
                        uti.1.insert(l.target.clone());
                    }
                }
            }
            if page.current_usage == Usage::Unused && page.next_usage != Usage::Unused {
                page.switch_to_next_usage();
            }

            // todo: mem::forget and panic in LockAttempt::drop()
        }
    }

    #[inline(never)]
    fn prepare_scheduled_execution(
        address_book: &mut AddressBook,
        unique_weight: UniqueWeight,
        task: TaskInQueue,
        finalized_lock_attempts: Vec<LockAttempt>,
    ) -> Box<ExecutionEnvironment> {
        let mut rng = rand::thread_rng();

        Box::new(ExecutionEnvironment {
            task,
            unique_weight,
            finalized_lock_attempts,
            is_reindexed: Default::default(),
            execution_result: Default::default(),
            thx: Default::default(),
            execution_us: Default::default(),
            execution_cpu_us: Default::default(),
            finish_time: Default::default(),
        })
    }

    #[inline(never)]
    fn commit_processed_execution(ee: &mut ExecutionEnvironment, address_book: &mut AddressBook) {
        ee.reindex_with_address_book();
        assert!(ee.is_reindexed());

        // which order for data race free?: unlocking / marking
        Self::unlock_after_execution(address_book, &mut ee.finalized_lock_attempts);
        ee.task.mark_as_finished();
    }

    #[inline(never)]
    fn schedule_next_execution(
        runnable_queue: &mut ModeSpecificTaskQueue,
        address_book: &mut AddressBook,
        task_selection: &mut TaskSelection,
        failed_lock_count: &mut usize,
    ) -> Option<Box<ExecutionEnvironment>> {
        let maybe_ee = Self::pop_from_queue_then_lock(
            runnable_queue,
            address_book,
            task_selection,
            failed_lock_count,
        )
        .map(|(uw, t, ll)| Self::prepare_scheduled_execution(address_book, uw, t, ll));
        maybe_ee
    }
}

impl<TH: Handler<SEA>, SEA: ScheduleExecutionArg> InstalledScheduler<SEA>
    for PooledScheduler<TH, SEA>
{
    fn id(&self) -> SchedulerId {
        self.id
    }

    fn context(&self) -> &SchedulingContext {
        self.context.as_ref().expect("active context should exist")
    }

    fn schedule_execution(&self, transaction_with_index: SEA::TransactionWithIndex<'_>) {
        let r = self.ensure_threads();
        let mut executing_queue_count = 0_usize;
        let mut provisioning_tracker_count = 0;
        let mut sequence_time = 0;
        let mut queue_clock = 0;
        let mut execute_clock = 0;
        let mut commit_clock = 0;
        let mut processed_count = 0_usize;
        let mut interval_count = 0;
        let mut failed_lock_count = 0;
        transaction_with_index.with_transaction_and_index(|transaction, index| {
            let locks = transaction.get_account_locks_unchecked();
            let writable_lock_iter = locks.writable.iter().map(|address| {
                LockAttempt::new(self.preloader.load(**address), RequestedUsage::Writable)
            });
            let readonly_lock_iter = locks.readonly.iter().map(|address| {
                LockAttempt::new(self.preloader.load(**address), RequestedUsage::Readonly)
            });
            let locks = writable_lock_iter
                .chain(readonly_lock_iter)
                .collect::<Vec<_>>();
            let uw = UniqueWeight::max_value() - index as UniqueWeight;
            let task = Task::new_for_queue(uw, (transaction.clone(), locks));
            let (transaction_sender, transaction_receiver) = crossbeam_channel::unbounded();
            let mut runnable_queue = ModeSpecificTaskQueue::BlockVerification(
                ChannelBackedTaskQueue::new(&transaction_receiver),
            );
            runnable_queue.add_to_schedule(task.unique_weight, task);
            let mut selection = TaskSelection::OnlyFromContended(usize::max_value());
            let mut address_book = self.address_book.lock().unwrap();
            let maybe_ee = ScheduleStage::schedule_next_execution(
                &mut runnable_queue,
                &mut address_book,
                &mut selection,
                &mut failed_lock_count,
            );
            if let Some(mut ee) = maybe_ee {
                ScheduleStage::commit_processed_execution(&mut ee, &mut address_book);
            }
        });
        drop(r);
    }

    fn wait_for_termination(&mut self, wait_reason: &WaitReason) -> Option<ResultWithTimings> {
        let keep_result_with_timings = wait_reason.is_paused();

        if keep_result_with_timings {
            None
        } else {
            drop(
                self.context
                    .take()
                    .expect("active context should be dropped"),
            );
            // current simplest form of this trait impl doesn't block the current thread materially
            // just with the following single mutex lock. Suppose more elaborated synchronization
            // across worker threads here in the future...
            self.result_with_timings
                .lock()
                .expect("not poisoned")
                .take()
        }
    }

    fn return_to_pool(self: Box<Self>) {
        self.pool.clone().return_scheduler(self)
    }
}

/*
struct SchedulingStateMachine {};

enum Event {
    New(Transaction),
    Executed(Transaction),
}

enum Action {
    Execute,
    Abort,
}

enum ActionResult {
    NoTransaction,
    Runnable(Transaction),
    Aborted,
}

impl SchedulingStateMachine {
    fn tick_by_event(Event) {}
    fn tick_by_action(Action) -> ActionResult {}
}

impl Thread
*/

impl<TH: Handler<SEA>, SEA: ScheduleExecutionArg> InstallableScheduler<SEA>
    for PooledScheduler<TH, SEA>
{
    fn has_context(&self) -> bool {
        self.context.is_some()
    }

    fn replace_context(&mut self, context: SchedulingContext) {
        self.context = Some(context);
        *self.result_with_timings.lock().expect("not poisoned") = None;
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

        fn context(&self) -> &SchedulingContext {
            self.0.context()
        }

        fn schedule_execution<'a>(
            &'a self,
            &(transaction, index): <DefaultScheduleExecutionArg as ScheduleExecutionArg>::TransactionWithIndex<'a>,
        ) {
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
        }

        fn wait_for_termination(&mut self, reason: &WaitReason) -> Option<ResultWithTimings> {
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
