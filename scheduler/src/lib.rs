#![feature(map_first_last)]

use {
    crossbeam_channel::{bounded, unbounded},
    log::*,
    rand::Rng,
    sha2::{Digest, Sha256},
    solana_measure::measure::Measure,
    solana_metrics::datapoint_info,
    solana_sdk::{
        hash::Hash,
        pubkey::Pubkey,
        transaction::{SanitizedTransaction, TransactionAccountLocks, VersionedTransaction},
    },
};

/*
type PageRcInner<T> = std::rc::Rc<T>;
unsafe impl Send for PageRc {}
*/

type PageRcInner<T> = triomphe::Arc<(std::cell::RefCell<Page<T>>, TaskIds<T>)>;

#[derive(Debug, Clone)]
pub struct PageRc<T>(PageRcInner<T>);
unsafe impl<T> Send for PageRc<T> {}
unsafe impl<T> Sync for PageRc<T> {}

type CU = u64;

#[derive(Debug)]
pub struct ExecutionEnvironment<T> {
    //accounts: Vec<i8>,
    pub cu: CU,
    pub unique_weight: UniqueWeight,
    pub task: TaskInQueue<T>,
    pub finalized_lock_attempts: Vec<LockAttempt<T>>,
    pub is_reindexed: bool,
    pub extra: T,
}

impl<T> ExecutionEnvironment<T> {
    //fn new(cu: usize) -> Self {
    //    Self {
    //        cu,
    //        ..Self::default()
    //    }
    //}

    //fn abort() {
    //  pass AtomicBool into InvokeContext??
    //}
    //
    #[inline(never)]
    pub fn reindex_with_address_book(&mut self) {
        assert!(!self.is_reindexed());
        self.is_reindexed = true;

        let uq = self.unique_weight;
        //self.task.trace_timestamps("in_exec(self)");
        let should_remove = self
            .task
            .contention_count
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0;
        for mut lock_attempt in self.finalized_lock_attempts.iter_mut() {
            let contended_unique_weights = lock_attempt.contended_unique_weights();
            contended_unique_weights
                .heaviest_task_cursor()
                .map(|mut task_cursor| {
                    let mut found = true;
                    let mut removed = false;
                    let mut task = task_cursor.value();
                    //task.trace_timestamps("in_exec(initial list)");
                    while !task.currently_contended() {
                        if task_cursor.key() == &uq {
                            assert!(should_remove);
                            removed = task_cursor.remove();
                            assert!(removed);
                        }
                        if task.already_finished() {
                            task_cursor.remove();
                        }
                        if let Some(new_cursor) = task_cursor.prev() {
                            assert!(new_cursor.key() < task_cursor.key());
                            task_cursor = new_cursor;
                            task = task_cursor.value();
                            //task.trace_timestamps("in_exec(subsequent list)");
                        } else {
                            found = false;
                            break;
                        }
                    }
                    if should_remove && !removed {
                        contended_unique_weights.remove_task(&uq);
                    }
                    found.then(|| Task::clone_in_queue(task))
                })
                .flatten()
                .map(|task| {
                    //task.trace_timestamps(&format!("in_exec(heaviest:{})", self.task.queue_time_label()));
                    lock_attempt.heaviest_uncontended = Some(task);
                    ()
                });
        }
    }

    fn is_reindexed(&self) -> bool {
        self.is_reindexed
    }
}

unsafe trait AtScheduleThread: Copy {}
pub unsafe trait NotAtScheduleThread: Copy {}

impl<T> PageRc<T> {
    fn page_mut<AST: AtScheduleThread>(&self, _ast: AST) -> std::cell::RefMut<'_, Page<T>> {
        self.0 .0.borrow_mut()
    }
}

#[derive(Clone, Debug)]
enum LockStatus {
    Succeded,
    Provisional,
    Failed,
}

#[derive(Debug)]
pub struct LockAttempt<T> {
    target: PageRc<T>,
    status: LockStatus,
    requested_usage: RequestedUsage,
    //pub heaviest_uncontended: arc_swap::ArcSwapOption<Task>,
    pub heaviest_uncontended: Option<TaskInQueue<T>>,
    //remembered: bool,
}

impl<T> LockAttempt<T> {
    pub fn new(target: PageRc<T>, requested_usage: RequestedUsage) -> Self {
        Self {
            target,
            status: LockStatus::Succeded,
            requested_usage,
            heaviest_uncontended: Default::default(),
            //remembered: false,
        }
    }

    pub fn clone_for_test(&self) -> Self {
        Self {
            target: self.target.clone(),
            status: LockStatus::Succeded,
            requested_usage: self.requested_usage,
            heaviest_uncontended: Default::default(),
            //remembered: false,
        }
    }

    pub fn contended_unique_weights<T>(&self) -> &TaskIds<T> {
        &self.target.0 .1
    }
}

type UsageCount = usize;
const SOLE_USE_COUNT: UsageCount = 1;

#[derive(Copy, Clone, Debug, PartialEq)]
enum Usage {
    Unused,
    // weight to abort running tx?
    // also sum all readonly weights to subvert to write lock with greater weight?
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

#[derive(Clone, Copy, Debug)]
pub enum RequestedUsage {
    Readonly,
    Writable,
}

#[derive(Debug, Default)]
pub struct TaskIds<T> {
    task_ids: crossbeam_skiplist::SkipMap<UniqueWeight, TaskInQueue<T>>,
}

impl<T> TaskIds<T> {
    #[inline(never)]
    pub fn insert_task<T>(&self, u: TaskId, task: TaskInQueue<T>) {
        let mut is_inserted = false;
        self.task_ids.get_or_insert_with(u, || {
            is_inserted = true;
            task
        });
        assert!(is_inserted);
    }

    #[inline(never)]
    pub fn remove_task(&self, u: &TaskId) {
        let removed_entry = self.task_ids.remove(u);
        //assert!(removed_entry.is_some());
    }

    #[inline(never)]
    pub fn heaviest_task_cursor<T>(
        &self,
    ) -> Option<crossbeam_skiplist::map::Entry<'_, UniqueWeight, TaskInQueue<T>>> {
        self.task_ids.back()
    }
}

#[derive(Debug)]
pub struct Page<T> {
    current_usage: Usage,
    next_usage: Usage,
    provisional_task_ids: Vec<triomphe::Arc<ProvisioningTracker<T>>>,
    cu: CU,
    //loaded account from Accounts db
    //comulative_cu for qos; i.e. track serialized cumulative keyed by addresses and bail out block
    //producing as soon as any one of cu from the executing thread reaches to the limit
}

impl<T> Page<T> {
    fn new(current_usage: Usage) -> Self {
        Self {
            current_usage,
            next_usage: Usage::Unused,
            provisional_task_ids: Default::default(),
            cu: Default::default(),
        }
    }

    fn switch_to_next_usage(&mut self) {
        self.current_usage = self.next_usage;
        self.next_usage = Usage::Unused;
    }
}

//type AddressMap = std::collections::HashMap<Pubkey, PageRc<T>>;
type AddressMap<T> = std::sync::Arc<dashmap::DashMap<Pubkey, PageRc<T>>>;
type TaskId = UniqueWeight;
type WeightedTaskIds<T> = std::collections::BTreeMap<TaskId, TaskInQueue<T>>;
//type AddressMapEntry<'a, K, V> = std::collections::hash_map::Entry<'a, K, V>;
type AddressMapEntry<'a, T> = dashmap::mapref::entry::Entry<'a, Pubkey, PageRc<T>>;

type StuckTaskId = (CU, TaskId);

// needs ttl mechanism and prune
#[derive(Default)]
pub struct AddressBook<T> {
    book: AddressMap,
    uncontended_task_ids: WeightedTaskIds<T>,
    fulfilled_provisional_task_ids: WeightedTaskIds<T>,
    stuck_tasks: std::collections::BTreeMap<StuckTaskId, TaskInQueue<T>>,
}

#[derive(Debug)]
struct ProvisioningTracker<T> {
    remaining_count: std::sync::atomic::AtomicUsize,
    task: TaskInQueue<T>,
}

impl<T> ProvisioningTracker<T> {
    fn new(remaining_count: usize, task: TaskInQueue<T>) -> Self {
        Self {
            remaining_count: std::sync::atomic::AtomicUsize::new(remaining_count),
            task,
        }
    }

    fn is_fulfilled(&self) -> bool {
        self.count() == 0
    }

    fn progress(&self) {
        self.remaining_count
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn prev_count(&self) -> usize {
        self.count() + 1
    }

    fn count(&self) -> usize {
        self.remaining_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl<T> AddressBook<T> {
    #[inline(never)]
    fn attempt_lock_address<AST: AtScheduleThread>(
        ast: AST,
        from_runnable: bool,
        prefer_immediate: bool,
        unique_weight: &UniqueWeight,
        attempt: &mut LockAttempt<T>,
    ) -> CU {
        let LockAttempt {
            target,
            requested_usage,
            status, /*, remembered*/
            ..
        } = attempt;

        let mut page = target.page_mut(ast);

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
                    if from_runnable || prefer_immediate {
                        *status = LockStatus::Failed;
                    } else {
                        match page.next_usage {
                            Usage::Unused => {
                                *status = LockStatus::Provisional;
                                page.next_usage = Usage::renew(*requested_usage);
                            }
                            // support multiple readonly locks!
                            Usage::Readonly(_) | Usage::Writable => {
                                *status = LockStatus::Failed;
                            }
                        }
                    }
                }
            },
            Usage::Writable => {
                if from_runnable || prefer_immediate {
                    *status = LockStatus::Failed;
                } else {
                    match page.next_usage {
                        Usage::Unused => {
                            *status = LockStatus::Provisional;
                            page.next_usage = Usage::renew(*requested_usage);
                        }
                        // support multiple readonly locks!
                        Usage::Readonly(_) | Usage::Writable => {
                            *status = LockStatus::Failed;
                        }
                    }
                }
            }
        }
        page.cu
    }

    fn reset_lock<AST: AtScheduleThread>(
        &mut self,
        ast: AST,
        attempt: &mut LockAttempt<T>,
        after_execution: bool,
    ) -> bool {
        match attempt.status {
            LockStatus::Succeded => self.unlock(ast, attempt),
            LockStatus::Provisional => {
                if after_execution {
                    self.unlock(ast, attempt)
                } else {
                    self.cancel(ast, attempt);
                    false
                }
            }
            LockStatus::Failed => {
                false // do nothing
            }
        }
    }

    #[inline(never)]
    fn unlock<AST: AtScheduleThread>(&mut self, ast: AST, attempt: &mut LockAttempt<T>) -> bool {
        //debug_assert!(attempt.is_success());

        let mut newly_uncontended = false;

        let mut page = attempt.target.page_mut(ast);

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
    fn cancel<AST: AtScheduleThread>(&mut self, ast: AST, attempt: &mut LockAttempt<T>) {
        let mut page = attempt.target.page_mut(ast);

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
pub struct Preloader<T> {
    book: AddressMap<T>,
}

impl<T> Preloader<T> {
    #[inline(never)]
    pub fn load(&self, address: Pubkey) -> PageRc<T> {
        PageRc::clone(&self.book.entry(address).or_insert_with(|| {
            PageRc(PageRcInner::new((
                core::cell::RefCell::new(Page::new(Usage::unused())),
                Default::default(),
            )))
        }))
    }
}

/*
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Weight {
    // naming: Sequence Ordering?
    pub ix: u64, // index in ledger entry?
                   // gas fee
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct UniqueWeight {
    // naming: Sequence Ordering?
    weight: Weight,
    // we can't use Transaction::message_hash because it's manipulatable to be favorous to the tx
    // submitter
    //unique_key: Hash, // tie breaker? random noise? also for unique identification of txes?
    unique_key: u64, // tie breaker? random noise? also for unique identification of txes?
    // fee?
}
*/
pub type Weight = u64;
pub type UniqueWeight = u64;

struct Bundle {
    // what about bundle1{tx1a, tx2} and bundle2{tx1b, tx2}?
}

#[derive(Debug)]
pub struct Task<T> {
    unique_weight: UniqueWeight,
    pub tx: (SanitizedTransaction, LockAttemptsInCell<T>), // actually should be Bundle
    pub contention_count: std::sync::atomic::AtomicUsize,
    pub busiest_page_cu: std::sync::atomic::AtomicU64,
    pub uncontended: std::sync::atomic::AtomicUsize,
    pub sequence_time: std::sync::atomic::AtomicUsize,
    pub sequence_end_time: std::sync::atomic::AtomicUsize,
    pub queue_time: std::sync::atomic::AtomicUsize,
    pub queue_end_time: std::sync::atomic::AtomicUsize,
    pub execute_time: std::sync::atomic::AtomicUsize,
    pub commit_time: std::sync::atomic::AtomicUsize,
    pub for_indexer: LockAttemptsInCell<T>,
    pub extra: T,
}

#[derive(Debug)]
pub struct LockAttemptsInCell<T>(std::cell::RefCell<Vec<LockAttempt<T>>>);

unsafe impl<T> Send for LockAttemptsInCell<T> {}
unsafe impl<T> Sync for LockAttemptsInCell<T> {}

impl<T> LockAttemptsInCell<T> {
    fn new(ll: std::cell::RefCell<Vec<LockAttempt<T>>>) -> Self {
        Self(ll)
    }
}

// sequence_time -> seq clock
// queue_time -> queue clock
// execute_time ---> exec clock
// commit_time  -+

impl<T> Task<T> {
    pub fn new_for_queue<NAST: NotAtScheduleThread>(
        nast: NAST,
        unique_weight: UniqueWeight,
        tx: (SanitizedTransaction, Vec<LockAttempt<T>>),
    ) -> TaskInQueue<T> {
        TaskInQueue::new(Self {
            for_indexer: LockAttemptsInCell::new(std::cell::RefCell::new(
                tx.1.iter().map(|a| a.clone_for_test()).collect(),
            )),
            unique_weight,
            tx: (tx.0, LockAttemptsInCell::new(std::cell::RefCell::new(tx.1))),
            contention_count: Default::default(),
            busiest_page_cu: Default::default(),
            uncontended: Default::default(),
            sequence_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            sequence_end_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            queue_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            queue_end_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            execute_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            commit_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
        })
    }

    #[inline(never)]
    pub fn clone_in_queue(this: &TaskInQueue<T>) -> TaskInQueue<T> {
        TaskInQueue::<T>::clone(this)
    }

    fn lock_attempts_mut<AST: AtScheduleThread>(
        &self,
        _ast: AST,
    ) -> std::cell::RefMut<'_, Vec<LockAttempt<T>>> {
        self.tx.1 .0.borrow_mut()
    }

    fn lock_attempts_not_mut<NAST: NotAtScheduleThread>(
        &self,
        _nast: NAST,
    ) -> std::cell::Ref<'_, Vec<LockAttempt<T>>> {
        self.tx.1 .0.borrow()
    }

    fn update_busiest_page_cu(&self, cu: CU) {
        self.busiest_page_cu
            .store(cu, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn record_sequence_time(&self, clock: usize) {
        //self.sequence_time.store(clock, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn sequence_time(&self) -> usize {
        self.sequence_time.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn sequence_end_time(&self) -> usize {
        self.sequence_end_time
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn record_queue_time(&self, seq_clock: usize, queue_clock: usize) {
        //self.sequence_end_time.store(seq_clock, std::sync::atomic::Ordering::SeqCst);
        //self.queue_time.store(queue_clock, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn queue_time(&self) -> usize {
        self.queue_time.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn queue_end_time(&self) -> usize {
        self.queue_end_time
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn record_execute_time(&self, queue_clock: usize, execute_clock: usize) {
        //self.queue_end_time.store(queue_clock, std::sync::atomic::Ordering::SeqCst);
        //self.execute_time.store(execute_clock, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn execute_time(&self) -> usize {
        self.execute_time.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn record_commit_time(&self, execute_clock: usize) {
        //self.commit_time.store(execute_clock, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn commit_time(&self) -> usize {
        self.commit_time.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn queue_time_label(&self) -> String {
        format!(
            "queue: [{}qT..{}qT; {}qD]",
            self.queue_time(),
            self.queue_end_time(),
            self.queue_end_time() - self.queue_time(),
        )
    }

    pub fn trace_timestamps(&self, prefix: &str) {
        trace!("{}: {:016x} seq: [{}sT..{}sT; {}sD], queue: [{}qT..{}qT; {}qD] exec: [{}eT..{}eT; {}eD]", 
              prefix,
              self.unique_weight,
              self.sequence_time(),
              self.sequence_end_time(),
              self.sequence_end_time() - self.sequence_time(),
              self.queue_time(), self.queue_end_time(), self.queue_end_time() - self.queue_time(),
              self.execute_time(), self.commit_time(), self.commit_time() - self.execute_time());
    }

    pub fn clone_for_test<NAST: NotAtScheduleThread>(&self, nast: NAST) -> Self {
        Self {
            unique_weight: self.unique_weight,
            for_indexer: LockAttemptsInCell::new(std::cell::RefCell::new(
                self.lock_attempts_not_mut(nast)
                    .iter()
                    .map(|a| a.clone_for_test())
                    .collect(),
            )),
            tx: (
                self.tx.0.clone(),
                LockAttemptsInCell::new(std::cell::RefCell::new(
                    self.lock_attempts_not_mut(nast)
                        .iter()
                        .map(|l| l.clone_for_test())
                        .collect::<Vec<_>>(),
                )),
            ),
            contention_count: Default::default(),
            busiest_page_cu: Default::default(),
            uncontended: Default::default(),
            sequence_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            sequence_end_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            queue_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            queue_end_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            execute_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
            commit_time: std::sync::atomic::AtomicUsize::new(usize::max_value()),
        }
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

    #[inline(never)]
    fn index_with_address_book(
        this: &TaskInQueue<T>,
        task_sender: &crossbeam_channel::Sender<(TaskInQueue<T>, Vec<LockAttempt<T>>)>,
    ) {
        //for lock_attempt in self.lock_attempts_mut(ast).iter() {
        //    lock_attempt.contended_unique_weights().insert_task(unique_weight, Task::clone_in_queue(&self));
        //}
        let a = Task::clone_in_queue(this);
        task_sender
            .send((a, std::mem::take(&mut *this.for_indexer.0.borrow_mut())))
            .unwrap();
    }

    fn stuck_task_id(&self) -> StuckTaskId {
        let cu = self
            .busiest_page_cu
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_ne!(cu, 0);
        (cu, TaskId::max_value() - self.unique_weight)
    }
}

// RunnableQueue, ContendedQueue?
#[derive(Default, Debug, Clone)]
pub struct TaskQueue<T> {
    tasks: std::collections::BTreeMap<UniqueWeight, TaskInQueue<T>>,
    //tasks: im::OrdMap<UniqueWeight, TaskInQueue>,
    //tasks: im::HashMap<UniqueWeight, TaskInQueue>,
    //tasks: std::sync::Arc<dashmap::DashMap<UniqueWeight, TaskInQueue>>,
}

pub type TaskInQueue<T> = triomphe::Arc<Task<T>>;

//type TaskQueueEntry<'a> = im::ordmap::Entry<'a, UniqueWeight, TaskInQueue>;
//type TaskQueueOccupiedEntry<'a> = im::ordmap::OccupiedEntry<'a, UniqueWeight, TaskInQueue>;
//type TaskQueueEntry<'a> = im::hashmap::Entry<'a, UniqueWeight, TaskInQueue, std::collections::hash_map::RandomState>;
//type TaskQueueOccupiedEntry<'a> = im::hashmap::OccupiedEntry<'a, UniqueWeight, TaskInQueue, std::collections::hash_map::RandomState>;
//type TaskQueueEntry<'a> = dashmap::mapref::entry::Entry<'a, UniqueWeight, TaskInQueue>;
//type TaskQueueOccupiedEntry<'a> = dashmap::mapref::entry::OccupiedEntry<'a, UniqueWeight, TaskInQueue, std::collections::hash_map::RandomState>;
type TaskQueueEntry<'a, T> = std::collections::btree_map::Entry<'a, UniqueWeight, TaskInQueue<T>>;
type TaskQueueOccupiedEntry<'a, T> =
    std::collections::btree_map::OccupiedEntry<'a, UniqueWeight, TaskInQueue<T>>;

impl<T> TaskQueue<T> {
    #[inline(never)]
    fn add_to_schedule(&mut self, unique_weight: UniqueWeight, task: TaskInQueue<T>) {
        //trace!("TaskQueue::add(): {:?}", unique_weight);
        let pre_existed = self.tasks.insert(unique_weight, task);
        assert!(pre_existed.is_none()); //, "identical shouldn't exist: {:?}", unique_weight);
    }

    #[inline(never)]
    fn heaviest_entry_to_execute(&mut self) -> Option<TaskQueueOccupiedEntry<'_, T>> {
        self.tasks.last_entry()
    }

    fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

#[inline(never)]
fn attempt_lock_for_execution<'a, AST: AtScheduleThread, T>(
    ast: AST,
    from_runnable: bool,
    prefer_immediate: bool,
    address_book: &mut AddressBook<T>,
    unique_weight: &UniqueWeight,
    message_hash: &'a Hash,
    lock_attempts: &mut [LockAttempt<T>],
) -> (usize, usize, CU) {
    // no short-cuircuit; we at least all need to add to the contended queue
    let mut unlockable_count = 0;
    let mut provisional_count = 0;
    let mut busiest_page_cu = 1;

    for attempt in lock_attempts.iter_mut() {
        let cu = AddressBook::<T>::attempt_lock_address(
            ast,
            from_runnable,
            prefer_immediate,
            unique_weight,
            attempt,
        );
        busiest_page_cu = busiest_page_cu.max(cu);

        match attempt.status {
            LockStatus::Succeded => {}
            LockStatus::Failed => {
                unlockable_count += 1;
            }
            LockStatus::Provisional => {
                provisional_count += 1;
            }
        }
    }

    (unlockable_count, provisional_count, busiest_page_cu)
}

type PreprocessedTransaction<T> = (SanitizedTransaction, Vec<LockAttempt<T>>);

/*
pub fn get_transaction_priority_details(tx: &SanitizedTransaction) -> u64 {
    use solana_program_runtime::compute_budget::ComputeBudget;
    let mut compute_budget = ComputeBudget::default();
    compute_budget
        .process_instructions(
            tx.message().program_instructions_iter(),
            true, // use default units per instruction
        )
        .map(|d| d.get_priority())
        .unwrap_or_default()
}
*/

pub struct ScheduleStage {}

#[derive(PartialEq, Eq)]
enum TaskSource {
    Runnable,
    Contended,
    Stuck,
}

impl ScheduleStage {
    fn push_to_runnable_queue<T>(task: TaskInQueue<T>, runnable_queue: &mut TaskQueue<T>) {
        runnable_queue.add_to_schedule(task.unique_weight, task);
    }

    #[inline(never)]
    fn get_heaviest_from_contended<'a, T>(
        address_book: &'a mut AddressBook<T>,
    ) -> Option<std::collections::btree_map::OccupiedEntry<'a, UniqueWeight, TaskInQueue<T>>> {
        address_book.uncontended_task_ids.last_entry()
    }

    #[inline(never)]
    fn select_next_task<'a, T>(
        runnable_queue: &'a mut TaskQueue<T>,
        address_book: &mut AddressBook<T>,
        contended_count: &usize,
    ) -> Option<(TaskSource, TaskInQueue<T>)> {
        match (
            runnable_queue.heaviest_entry_to_execute(),
            Self::get_heaviest_from_contended(address_book),
        ) {
            (Some(heaviest_runnable_entry), None) => {
                trace!("select: runnable only");
                let t = heaviest_runnable_entry.remove();
                Some((TaskSource::Runnable, t))
            }
            (None, Some(weight_from_contended)) => {
                trace!("select: contended only");
                let t = weight_from_contended.remove();
                Some((TaskSource::Contended, t))
            }
            (Some(heaviest_runnable_entry), Some(weight_from_contended)) => {
                let weight_from_runnable = heaviest_runnable_entry.key();
                let uw = weight_from_contended.key();

                if weight_from_runnable > uw {
                    trace!("select: runnable > contended");
                    let t = heaviest_runnable_entry.remove();
                    Some((TaskSource::Runnable, t))
                } else if uw > weight_from_runnable {
                    trace!("select: contended > runnnable");
                    let t = weight_from_contended.remove();
                    Some((TaskSource::Contended, t))
                } else {
                    unreachable!(
                        "identical unique weights shouldn't exist in both runnable and contended"
                    )
                }
            }
            (None, None) => {
                trace!("select: none");
                if runnable_queue.task_count() == 0 && /* *contended_count > 0 &&*/ address_book.stuck_tasks.len() > 0
                {
                    trace!("handling stuck...");
                    let (stuck_task_id, task) = address_book.stuck_tasks.pop_first().unwrap();
                    // ensure proper rekeying
                    assert_eq!(task.stuck_task_id(), stuck_task_id);

                    if task.currently_contended() {
                        Some((TaskSource::Stuck, task))
                    } else {
                        // is it expected for uncontended tasks is in the stuck queue, to begin
                        // with??
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    #[inline(never)]
    fn pop_from_queue_then_lock<AST: AtScheduleThread, T>(
        ast: AST,
        task_sender: &crossbeam_channel::Sender<(TaskInQueue<T>, Vec<LockAttempt<T>>)>,
        runnable_queue: &mut TaskQueue<T>,
        address_book: &mut AddressBook<T>,
        contended_count: &mut usize,
        prefer_immediate: bool,
        sequence_clock: &usize,
        queue_clock: &mut usize,
        provisioning_tracker_count: &mut usize,
    ) -> Option<(UniqueWeight, TaskInQueue<T>, Vec<LockAttempt<T>>)> {
        if let Some(mut a) = address_book.fulfilled_provisional_task_ids.pop_last() {
            trace!(
                "expediate pop from provisional queue [rest: {}]",
                address_book.fulfilled_provisional_task_ids.len()
            );

            let lock_attempts = std::mem::take(&mut *a.1.lock_attempts_mut(ast));

            return Some((a.0, a.1, lock_attempts));
        }

        trace!("pop begin");
        loop {
            if let Some((task_source, mut next_task)) =
                Self::select_next_task(runnable_queue, address_book, contended_count)
            {
                trace!("pop loop iteration");
                let from_runnable = task_source == TaskSource::Runnable;
                if from_runnable {
                    next_task.record_queue_time(*sequence_clock, *queue_clock);
                    *queue_clock = queue_clock.checked_add(1).unwrap();
                }
                let unique_weight = next_task.unique_weight;
                let message_hash = next_task.tx.0.message_hash();

                // plumb message_hash into StatusCache or implmenent our own for duplicate tx
                // detection?

                let (unlockable_count, provisional_count, busiest_page_cu) =
                    attempt_lock_for_execution(
                        ast,
                        from_runnable,
                        prefer_immediate,
                        address_book,
                        &unique_weight,
                        &message_hash,
                        &mut next_task.lock_attempts_mut(ast),
                    );

                if unlockable_count > 0 {
                    //trace!("reset_lock_for_failed_execution(): {:?} {}", (&unique_weight, from_runnable), next_task.tx.0.signature());
                    Self::reset_lock_for_failed_execution(
                        ast,
                        address_book,
                        &unique_weight,
                        &mut next_task.lock_attempts_mut(ast),
                    );
                    let lock_count = next_task.lock_attempts_mut(ast).len();
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
                        *contended_count = contended_count.checked_add(1).unwrap();

                        Task::index_with_address_book(&next_task, task_sender);

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

                    if from_runnable || task_source == TaskSource::Stuck {
                        // for the case of being struck, we have already removed it from
                        // stuck_tasks, so pretend to add anew one.
                        // todo: optimize this needless operation
                        next_task.update_busiest_page_cu(busiest_page_cu);
                        let a = address_book
                            .stuck_tasks
                            .insert(next_task.stuck_task_id(), Task::clone_in_queue(&next_task));
                        assert!(a.is_none());
                        if from_runnable {
                            continue; // continue to prefer depleting the possibly-non-empty runnable queue
                        } else if task_source == TaskSource::Stuck {
                            // need to bail out immediately to avoid going to infinite loop of re-processing
                            // the struck task again.
                            // todo?: buffer restuck tasks until readd to the stuck tasks until
                            // some scheduling state tick happens and try 2nd idling stuck task in
                            // the collection?
                            break;
                        } else {
                            unreachable!();
                        }
                    } else {
                        // todo: remove this task from stuck_tasks before update_busiest_page_cu
                        let removed = address_book
                            .stuck_tasks
                            .remove(&next_task.stuck_task_id())
                            .unwrap();
                        next_task.update_busiest_page_cu(busiest_page_cu);
                        let a = address_book
                            .stuck_tasks
                            .insert(next_task.stuck_task_id(), removed);
                        assert!(a.is_none());
                        break;
                    }
                } else if provisional_count > 0 {
                    assert!(!from_runnable);
                    assert_eq!(unlockable_count, 0);
                    let lock_count = next_task.lock_attempts_mut(ast).len();
                    trace!("provisional exec: [{}/{}]", provisional_count, lock_count);
                    *contended_count = contended_count.checked_sub(1).unwrap();
                    next_task.mark_as_uncontended();
                    address_book.stuck_tasks.remove(&next_task.stuck_task_id());
                    next_task.update_busiest_page_cu(busiest_page_cu);

                    let tracker = triomphe::Arc::new(ProvisioningTracker::new(
                        provisional_count,
                        Task::clone_in_queue(&next_task),
                    ));
                    *provisioning_tracker_count =
                        provisioning_tracker_count.checked_add(1).unwrap();
                    Self::finalize_lock_for_provisional_execution(
                        ast,
                        address_book,
                        &next_task,
                        tracker,
                    );

                    break;
                    continue;
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
                    *contended_count = contended_count.checked_sub(1).unwrap();
                    next_task.mark_as_uncontended();
                } else {
                    next_task.update_busiest_page_cu(busiest_page_cu);
                }
                let lock_attempts = std::mem::take(&mut *next_task.lock_attempts_mut(ast));

                return Some((unique_weight, next_task, lock_attempts));
            } else {
                break;
            }
        }

        None
    }

    #[inline(never)]
    fn finalize_lock_for_provisional_execution<AST: AtScheduleThread, T>(
        ast: AST,
        address_book: &mut AddressBook<T>,
        next_task: &Task<T>,
        tracker: triomphe::Arc<ProvisioningTracker<T>>,
    ) {
        for l in next_task.lock_attempts_mut(ast).iter_mut() {
            match l.status {
                LockStatus::Provisional => {
                    l.target
                        .page_mut(ast)
                        .provisional_task_ids
                        .push(triomphe::Arc::clone(&tracker));
                }
                LockStatus::Succeded => {
                    // do nothing
                }
                LockStatus::Failed => {
                    unreachable!();
                }
            }
        }
        //trace!("provisioning_trackers: {}", address_book.provisioning_trackers.len());
    }

    #[inline(never)]
    fn reset_lock_for_failed_execution<AST: AtScheduleThread, T>(
        ast: AST,
        address_book: &mut AddressBook<T>,
        unique_weight: &UniqueWeight,
        lock_attempts: &mut [LockAttempt<T>],
    ) {
        for l in lock_attempts {
            address_book.reset_lock(ast, l, false);
        }
    }

    #[inline(never)]
    fn unlock_after_execution<AST: AtScheduleThread, T>(
        ast: AST,
        address_book: &mut AddressBook<T>,
        lock_attempts: &mut [LockAttempt<T>],
        provisioning_tracker_count: &mut usize,
        cu: CU,
    ) {
        for mut l in lock_attempts {
            let newly_uncontended = address_book.reset_lock(ast, &mut l, true);

            let mut page = l.target.page_mut(ast);
            page.cu += cu;
            if newly_uncontended && page.next_usage == Usage::Unused {
                //let mut inserted = false;

                if let Some(task) = l.heaviest_uncontended.take() {
                    //assert!(!task.already_finished());
                    if
                    /*true ||*/
                    task.currently_contended() {
                        //assert!(task.currently_contended());
                        //inserted = true;
                        address_book
                            .uncontended_task_ids
                            .insert(task.unique_weight, task);
                    } /*else {
                          let contended_unique_weights = &page.contended_unique_weights;
                          contended_unique_weights.heaviest_task_cursor().map(|mut task_cursor| {
                              let mut found = true;
                              //assert_ne!(task_cursor.key(), &task.uq);
                              let mut task = task_cursor.value();
                              while !task.currently_contended() {
                                  if let Some(new_cursor) = task_cursor.prev() {
                                      assert!(new_cursor.key() < task_cursor.key());
                                      //assert_ne!(new_cursor.key(), &uq);
                                      task_cursor = new_cursor;
                                      task = task_cursor.value();
                                  } else {
                                      found = false;
                                      break;
                                  }
                              }
                              found.then(|| Task::clone_in_queue(task))
                          }).flatten().map(|task| {
                              address_book.uncontended_task_ids.insert(task.unique_weight, task);
                              ()
                          });
                      }*/
                }
            }
            if page.current_usage == Usage::Unused && page.next_usage != Usage::Unused {
                page.switch_to_next_usage();
                for tracker in std::mem::take(&mut page.provisional_task_ids).into_iter() {
                    tracker.progress();
                    if tracker.is_fulfilled() {
                        trace!(
                            "provisioning tracker progress: {} => {} (!)",
                            tracker.prev_count(),
                            tracker.count()
                        );
                        address_book.fulfilled_provisional_task_ids.insert(
                            tracker.task.unique_weight,
                            Task::clone_in_queue(&tracker.task),
                        );
                        *provisioning_tracker_count =
                            provisioning_tracker_count.checked_sub(1).unwrap();
                    } else {
                        trace!(
                            "provisioning tracker progress: {} => {}",
                            tracker.prev_count(),
                            tracker.count()
                        );
                    }
                }
            }

            // todo: mem::forget and panic in LockAttempt<T>::drop()
        }
    }

    #[inline(never)]
    fn prepare_scheduled_execution<T>(
        address_book: &mut AddressBook<T>,
        unique_weight: UniqueWeight,
        task: TaskInQueue<T>,
        finalized_lock_attempts: Vec<LockAttempt<T>>,
        queue_clock: &usize,
        execute_clock: &mut usize,
    ) -> Box<ExecutionEnvironment<T>> {
        let mut rng = rand::thread_rng();
        // load account now from AccountsDb
        task.record_execute_time(*queue_clock, *execute_clock);
        *execute_clock = execute_clock.checked_add(1).unwrap();

        Box::new(ExecutionEnvironment {
            task,
            unique_weight,
            cu: rng.gen_range(3, 1000),
            finalized_lock_attempts,
            is_reindexed: Default::default(),
        })
    }

    #[inline(never)]
    fn commit_completed_execution<AST: AtScheduleThread, T>(
        ast: AST,
        ee: &mut ExecutionEnvironment<T>,
        address_book: &mut AddressBook<T>,
        commit_time: &mut usize,
        provisioning_tracker_count: &mut usize,
    ) {
        // do par()-ly?

        //ee.reindex_with_address_book();
        assert!(ee.is_reindexed());

        ee.task.record_commit_time(*commit_time);
        //ee.task.trace_timestamps("commit");
        //*commit_time = commit_time.checked_add(1).unwrap();

        // which order for data race free?: unlocking / marking
        Self::unlock_after_execution(
            ast,
            address_book,
            &mut ee.finalized_lock_attempts,
            provisioning_tracker_count,
            ee.cu,
        );
        ee.task.mark_as_finished();

        address_book.stuck_tasks.remove(&ee.task.stuck_task_id());

        // block-wide qos validation will be done here
        // if error risen..:
        //   don't commit the tx for banking and potentially finish scheduling at block max cu
        //   limit
        //   mark the block as dead for replaying

        // par()-ly clone updated Accounts into address book
    }

    #[inline(never)]
    fn schedule_next_execution<AST: AtScheduleThread, T>(
        ast: AST,
        task_sender: &crossbeam_channel::Sender<(TaskInQueue<T>, Vec<LockAttempt<T>>)>,
        runnable_queue: &mut TaskQueue<T>,
        address_book: &mut AddressBook<T>,
        contended_count: &mut usize,
        prefer_immediate: bool,
        sequence_time: &usize,
        queue_clock: &mut usize,
        execute_clock: &mut usize,
        provisioning_tracker_count: &mut usize,
    ) -> Option<Box<ExecutionEnvironment<T>>> {
        let maybe_ee = Self::pop_from_queue_then_lock(
            ast,
            task_sender,
            runnable_queue,
            address_book,
            contended_count,
            prefer_immediate,
            sequence_time,
            queue_clock,
            provisioning_tracker_count,
        )
        .map(|(uw, t, ll)| {
            Self::prepare_scheduled_execution(address_book, uw, t, ll, queue_clock, execute_clock)
        });
        maybe_ee
    }

    #[inline(never)]
    fn register_runnable_task<T>(
        weighted_tx: TaskInQueue<T>,
        runnable_queue: &mut TaskQueue<T>,
        sequence_time: &mut usize,
    ) {
        weighted_tx.record_sequence_time(*sequence_time);
        *sequence_time = sequence_time.checked_add(1).unwrap();
        Self::push_to_runnable_queue(weighted_tx, runnable_queue)
    }

    fn _run<AST: AtScheduleThread, T>(
        ast: AST,
        max_executing_queue_count: usize,
        runnable_queue: &mut TaskQueue<T>,
        address_book: &mut AddressBook<T>,
        from_prev: &crossbeam_channel::Receiver<SchedulablePayload<T>>,
        to_execute_substage: &crossbeam_channel::Sender<ExecutablePayload<T>>,
        from_exec: &crossbeam_channel::Receiver<UnlockablePayload<T>>,
        maybe_to_next_stage: Option<&crossbeam_channel::Sender<DroppablePayload<T>>>, // assume nonblocking
    ) {
        let random_id = rand::thread_rng().gen::<u64>();
        info!("schedule_once:initial id_{:016x}", random_id);

        let mut executing_queue_count = 0_usize;
        let mut contended_count = 0;
        let mut provisioning_tracker_count = 0;
        let mut sequence_time = 0;
        let mut queue_clock = 0;
        let mut execute_clock = 0;
        let mut completed_count = 0_usize;
        assert!(max_executing_queue_count > 0);

        let (ee_sender, ee_receiver) = crossbeam_channel::unbounded::<DroppablePayload>();

        let (to_next_stage, maybe_reaper_thread_handle) = if let Some(to_next_stage) = maybe_to_next_stage {
            (to_next_stage, None)
        } else {
            let h = std::thread::Builder::new()
                .name("solReaper".to_string())
                .spawn(move || {
                    #[derive(Clone, Copy, Debug)]
                    struct NotAtTopOfScheduleThread;
                    unsafe impl NotAtScheduleThread for NotAtTopOfScheduleThread {}
                    let nast = NotAtTopOfScheduleThread;

                    while let Ok(DroppablePayload(mut a)) = ee_receiver.recv() {
                        assert!(a.task.lock_attempts_not_mut(nast).is_empty());
                        //assert!(a.task.sequence_time() != usize::max_value());
                        //let lock_attempts = std::mem::take(&mut a.lock_attempts);
                        //drop(lock_attempts);
                        //TaskInQueue::get_mut(&mut a.task).unwrap();
                    }
                    assert_eq!(ee_receiver.len(), 0);
                    Ok::<(), ()>(())
                })
                .unwrap();

            (&ee_sender, Some(h))
        };
        let (task_sender, task_receiver) =
            crossbeam_channel::unbounded::<(TaskInQueue, Vec<LockAttempt<T>>)>();
        let indexer_count = std::env::var("INDEXER_COUNT")
            .unwrap_or(format!("{}", 4))
            .parse::<usize>()
            .unwrap();
        let indexer_handles = (0..indexer_count).map(|thx| {
            let task_receiver = task_receiver.clone();
            std::thread::Builder::new()
                .name(format!("solIndexer{:02}", thx))
                .spawn(move || {
                    while let Ok((task, ll)) = task_receiver.recv() {
                        for lock_attempt in ll {
                            if task.already_finished() {
                                break;
                            }
                            lock_attempt
                                .contended_unique_weights()
                                .insert_task(task.unique_weight, Task::clone_in_queue(&task));
                        }
                    }
                    assert_eq!(task_receiver.len(), 0);
                    Ok::<(), ()>(())
                })
                .unwrap()
        }).collect::<Vec<_>>();
        let mut start = std::time::Instant::now();

        let (mut from_disconnected, mut from_exec_disconnected, mut no_more_work) = Default::default();
        loop {
            crossbeam_channel::select! {
               recv(from_exec) -> maybe_from_exec => {
                   if maybe_from_exec.is_err() {
                       assert_eq!(from_exec.len(), 0);
                       from_exec_disconnected |= true;
                       if from_disconnected {
                           break;
                       } else {
                           continue;
                       }
                   }
                   let mut processed_execution_environment = maybe_from_exec.unwrap().0;
                    executing_queue_count = executing_queue_count.checked_sub(1).unwrap();
                    completed_count = completed_count.checked_add(1).unwrap();
                    Self::commit_completed_execution(ast, &mut processed_execution_environment, address_book, &mut execute_clock, &mut provisioning_tracker_count);
                    to_next_stage.send(DroppablePayload(processed_execution_environment)).unwrap();
               }
               recv(from_prev) -> maybe_from => {
                   if maybe_from.is_err() {
                       assert_eq!(from_prev.len(), 0);
                       from_disconnected |= true;
                       no_more_work |= runnable_queue.task_count() + contended_count + executing_queue_count + provisioning_tracker_count == 0;
                       if from_exec_disconnected || no_more_work {
                           break;
                       } else {
                           continue;
                       }
                   }
                   let task = maybe_from.unwrap().0;
                   Self::register_runnable_task(task, runnable_queue, &mut sequence_time);
               }
            }

            let mut first_iteration = true;
            let (mut empty_from, mut empty_from_exec) = (false, false);
            let (mut from_len, mut from_exec_len) = (0, 0);

            loop {
                while (executing_queue_count + provisioning_tracker_count)
                    < max_executing_queue_count
                {
                    trace!("schedule_once (from: {}, to: {}, runnnable: {}, contended: {}, (immediate+provisional)/max: ({}+{})/{}) active from contended: {} stuck: {} completed: {}!", from_prev.len(), to_execute_substage.len(), runnable_queue.task_count(), contended_count, executing_queue_count, provisioning_tracker_count, max_executing_queue_count, address_book.uncontended_task_ids.len(), address_book.stuck_tasks.len(), completed_count);
                    if start.elapsed() > std::time::Duration::from_millis(1000) {
                        start = std::time::Instant::now();
                        info!("schedule_once:interval (from: {}, to: {}, runnnable: {}, contended: {}, (immediate+provisional)/max: ({}+{})/{}) active from contended: {} stuck: {} completed: {}!", from_prev.len(), to_execute_substage.len(), runnable_queue.task_count(), contended_count, executing_queue_count, provisioning_tracker_count, max_executing_queue_count, address_book.uncontended_task_ids.len(), address_book.stuck_tasks.len(), completed_count);
                    }
                    let prefer_immediate = provisioning_tracker_count / 4 > executing_queue_count;
                    if let Some(ee) = Self::schedule_next_execution(
                        ast,
                        &task_sender,
                        runnable_queue,
                        address_book,
                        &mut contended_count,
                        prefer_immediate,
                        &sequence_time,
                        &mut queue_clock,
                        &mut execute_clock,
                        &mut provisioning_tracker_count,
                    ) {
                        executing_queue_count = executing_queue_count.checked_add(1).unwrap();
                        to_execute_substage.send(ExecutablePayload(ee)).unwrap();
                    } else {
                        break;
                    }
                }
                //break;
                if first_iteration {
                    first_iteration = false;
                    (from_len, from_exec_len) = (from_prev.len(), from_exec.len());
                } else {
                    if empty_from {
                        from_len = from_prev.len();
                    }
                    if empty_from_exec {
                        from_exec_len = from_exec.len();
                    }
                }
                (empty_from, empty_from_exec) = (from_len == 0, from_exec_len == 0);

                if empty_from && empty_from_exec {
                    break;
                } else {
                    if !empty_from_exec {
                        let mut processed_execution_environment = from_exec.recv().unwrap().0;
                        from_exec_len = from_exec_len.checked_sub(1).unwrap();
                        empty_from_exec = from_exec_len == 0;
                        executing_queue_count = executing_queue_count.checked_sub(1).unwrap();
                        completed_count = completed_count.checked_add(1).unwrap();
                        Self::commit_completed_execution(
                            ast,
                            &mut processed_execution_environment,
                            address_book,
                            &mut execute_clock,
                            &mut provisioning_tracker_count,
                        );
                        to_next_stage.send(DroppablePayload(processed_execution_environment)).unwrap();
                    }
                    if !empty_from {
                        let task = from_prev.recv().unwrap().0;
                        from_len = from_len.checked_sub(1).unwrap();
                        empty_from = from_len == 0;
                        Self::register_runnable_task(task, runnable_queue, &mut sequence_time);
                    }
                }
            }
        }
        drop(to_next_stage);
        drop(ee_sender);
        drop(task_sender);
        drop(task_receiver);
        info!("run finished...");
        if let Some(h) = maybe_reaper_thread_handle {
            h.join().unwrap().unwrap();
        }
        for indexer_handle in indexer_handles {
            indexer_handle.join().unwrap().unwrap();
        }

        info!("schedule_once:final id_{:016x} (from_disconnected: {}, from_exec_disconnected: {}, no_more_work: {}) (from: {}, to: {}, runnnable: {}, contended: {}, (immediate+provisional)/max: ({}+{})/{}) active from contended: {} stuck: {} completed: {}!", random_id, from_disconnected, from_exec_disconnected, no_more_work, from_prev.len(), to_execute_substage.len(), runnable_queue.task_count(), contended_count, executing_queue_count, provisioning_tracker_count, max_executing_queue_count, address_book.uncontended_task_ids.len(), address_book.stuck_tasks.len(), completed_count);
    }

    pub fn run<T>(
        max_executing_queue_count: usize,
        runnable_queue: &mut TaskQueue<T>,
        address_book: &mut AddressBook<T>,
        from: &crossbeam_channel::Receiver<SchedulablePayload<T>>,
        to_execute_substage: &crossbeam_channel::Sender<ExecutablePayload<T>>,
        from_execute_substage: &crossbeam_channel::Receiver<UnlockablePayload<T>>,
        maybe_to_next_stage: Option<&crossbeam_channel::Sender<DroppablePayload<T>>>, // assume nonblocking
    ) {
        #[derive(Clone, Copy, Debug)]
        struct AtTopOfScheduleThread;
        unsafe impl AtScheduleThread for AtTopOfScheduleThread {}

        Self::_run::<AtTopOfScheduleThread>(
            AtTopOfScheduleThread,
            max_executing_queue_count,
            runnable_queue,
            address_book,
            from,
            to_execute_substage,
            from_execute_substage,
            maybe_to_next_stage,
        )
    }
}

pub struct SchedulablePayload<T>(pub TaskInQueue<T>);
pub struct ExecutablePayload<T>(pub Box<ExecutionEnvironment<T>>);
pub struct UnlockablePayload<T>(pub Box<ExecutionEnvironment<T>>);
pub struct DroppablePayload<T>(pub Box<ExecutionEnvironment<T>>);

struct ExecuteStage {
    //bank: Bank,
}

impl ExecuteStage {}
