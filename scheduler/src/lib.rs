#![feature(map_first_last)]
#![feature(get_mut_unchecked)]

use {
    atomic_enum::atomic_enum,
    crossbeam_channel::{bounded, unbounded},
    log::*,
    rand::Rng,
    sha2::{Digest, Sha256},
    solana_entry::entry::Entry,
    solana_measure::measure::Measure,
    solana_metrics::datapoint_info,
    solana_sdk::{
        hash::Hash,
        pubkey::Pubkey,
        transaction::{SanitizedTransaction, TransactionAccountLocks, VersionedTransaction},
    },
};

/*
type MyRcInner<T> = std::rc::Rc<T>;
unsafe impl Send for PageRc {}
*/

type MyRcInner = std::sync::Arc<Page>;

#[derive(Debug, Clone)]
pub struct PageRc(MyRcInner);


#[derive(Debug)]
pub struct ExecutionEnvironment {
    //accounts: Vec<i8>,
    pub cu: usize,
    pub unique_weight: UniqueWeight,
    pub task: TaskInQueue,
}

impl ExecutionEnvironment {
    //fn new(cu: usize) -> Self {
    //    Self {
    //        cu,
    //        ..Self::default()
    //    }
    //}

    //fn abort() {
    //  pass AtomicBool into InvokeContext??
    //}
}

impl PageRc {
    fn page_mut(&mut self) -> &mut Page {
        unsafe { MyRcInner::get_mut_unchecked(&mut self.0) }
    }

    pub fn page_ref(&self) -> &Page {
        <MyRcInner as std::borrow::Borrow<_>>::borrow(&self.0)
    }
}

#[derive(Clone, Debug)]
enum LockStatus {
    Succeded,
    Provisional, 
    Failed,
}

#[derive(Debug)]
pub struct LockAttempt {
    pub target: PageRc,
    status: LockStatus,
    requested_usage: RequestedUsage,
    pub heaviest_uncontended: arc_swap::ArcSwapOption<Task>,
    //remembered: bool,
}

impl LockAttempt {
    pub fn new(target: PageRc, requested_usage: RequestedUsage) -> Self {
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
pub struct TaskIds {
    //task_ids: std::collections::BTreeSet<UniqueWeight>,
    task_ids: crossbeam_skiplist::SkipMap<UniqueWeight, TaskInQueue>,
    //cached_heaviest: Option<UniqueWeight>,
}

impl TaskIds {
    #[inline(never)]
    pub fn insert_task_id(&self, u: TaskId, task: TaskInQueue) {
        /*
        match self.cached_heaviest {
            Some(c) if u > c => { self.cached_heaviest = Some(u) },
            None => { self.cached_heaviest = Some(u); }
            _ => {},
        }
        */
             
        self.task_ids.insert(u, task);
    }

    #[inline(never)]
    pub fn remove_task_id(&self, u: &TaskId) {
        let a = self.task_ids.remove(u);
        /*
        match self.cached_heaviest {
            //Some(ref c) if u == c => { self.cached_heaviest = self.task_ids.last().copied() },
            Some(ref c) if u == c => { self.cached_heaviest = self.task_ids.back().map(|e| *(e.value())) },
            _ => {},
        }
        */
        a;
    }

    #[inline(never)]
    fn has_more(&self) -> bool {
        !self.task_ids.is_empty()
    }

    #[inline(never)]
    fn heaviest_task_id(&self) -> Option<TaskId> {
        //self.task_ids.last()
        self.task_ids.back().map(|e| *(e.key()))
        //self.cached_heaviest.as_ref()
    }

    #[inline(never)]
    pub fn heaviest_task_cursor(&self) -> Option<crossbeam_skiplist::map::Entry<'_, UniqueWeight, TaskInQueue>> {
        //self.task_ids.last()
        self.task_ids.back()
        //self.cached_heaviest.as_ref()
    }
}

#[derive(Debug)]
pub struct Page {
    current_usage: Usage,
    next_usage: Usage,
    pub contended_unique_weights: TaskIds,
    provisional_task_ids: std::collections::HashSet<TaskId>,
    //loaded account from Accounts db
    //comulative_cu for qos; i.e. track serialized cumulative keyed by addresses and bail out block
    //producing as soon as any one of cu from the executing thread reaches to the limit
}

impl Page {
    fn new(current_usage: Usage) -> Self {
        Self {
            current_usage,
            next_usage: Usage::Unused,
            contended_unique_weights: Default::default(),
            provisional_task_ids: Default::default(),
        }
    }

    fn switch_to_next_usage(&mut self) {
        self.current_usage = self.next_usage;
        self.next_usage = Usage::Unused;
    }
}

//type AddressMap = std::collections::HashMap<Pubkey, PageRc>;
type AddressMap = std::sync::Arc<dashmap::DashMap<Pubkey, PageRc>>;
type TaskId = UniqueWeight;
type WeightedTaskIds = std::collections::BTreeMap<TaskId, TaskInQueue>;
//type AddressMapEntry<'a, K, V> = std::collections::hash_map::Entry<'a, K, V>;
type AddressMapEntry<'a> = dashmap::mapref::entry::Entry<'a, Pubkey, PageRc>;

// needs ttl mechanism and prune
#[derive(Default)]
pub struct AddressBook {
    book: AddressMap,
    uncontended_task_ids: WeightedTaskIds,
    provisioning_trackers: std::collections::HashMap<UniqueWeight, (ProvisioningTracker, TaskInQueue)>, 
    fulfilled_provisional_task_ids: WeightedTaskIds,
}

struct ProvisioningTracker {
    remaining_count: usize,
}

impl ProvisioningTracker {
    fn new(remaining_count: usize) -> Self {
        Self { remaining_count }
    }

    fn is_fulfilled(&self) -> bool {
        self.remaining_count == 0
    }

    fn tick(&mut self) {
        self.remaining_count = self.remaining_count.checked_sub(1).unwrap();
    }

    fn prev_count(&self) -> usize {
        self.remaining_count + 1
    }

    fn count(&self) -> usize {
        self.remaining_count
    }
}

impl AddressBook {
    #[inline(never)]
    fn attempt_lock_address(
        from_runnable: bool,
        prefer_immediate: bool,
        unique_weight: &UniqueWeight,
        attempt: &mut LockAttempt,
    ) {
        let LockAttempt {target, requested_usage, status/*, remembered*/, ..} = attempt;

                let mut page = target.page_mut();

                match page.current_usage {
                    Usage::Unused => {
                        assert_eq!(page.next_usage, Usage::Unused);
                        page.current_usage = Usage::renew(*requested_usage);
                        *status = LockStatus::Succeded;
                    }
                    Usage::Readonly(ref mut count) => match requested_usage {
                        RequestedUsage::Readonly => {
                            // prevent newer read-locks (even from runnable too)
                            match page.next_usage {
                                Usage::Unused => {
                                    *count += 1;
                                    *status = LockStatus::Succeded;
                                },
                                Usage::Readonly(_) | Usage::Writable => {
                                    *status = LockStatus::Failed;
                                }
                            }
                        }
                        RequestedUsage::Writable => {
                            if from_runnable || prefer_immediate {
                                //Self::remember_address_contention(&mut page, unique_weight);
                                //*remembered = true;
                                *status = LockStatus::Failed;
                            } else {
                                match page.next_usage {
                                    Usage::Unused => {
                                        *status = LockStatus::Provisional;
                                        page.next_usage = Usage::renew(*requested_usage);
                                    },
                                    // support multiple readonly locks!
                                    Usage::Readonly(_) | Usage::Writable => {
                                        *status = LockStatus::Failed;
                                    },
                                }
                                
                                //*status = LockStatus::Failed;
                            }
                        }
                    },
                    Usage::Writable => {
                        if from_runnable || prefer_immediate {
                            //Self::remember_address_contention(&mut page, unique_weight);
                            //*remembered = true;
                            *status = LockStatus::Failed;
                        } else {
                            match page.next_usage {
                                Usage::Unused => {
                                    *status = LockStatus::Provisional;
                                    page.next_usage = Usage::renew(*requested_usage);
                                },
                                // support multiple readonly locks!
                                Usage::Readonly(_) | Usage::Writable => {
                                    *status = LockStatus::Failed;
                                },
                            }
                            //*status = LockStatus::Failed;
                        }
                    },
                }
    }

    /*
    #[inline(never)]
    fn remember_address_contention(page: &mut Page, unique_weight: &UniqueWeight) {
        page.contended_unique_weights.insert_task_id(*unique_weight);
    }

    #[inline(never)]
    fn forget_address_contention(unique_weight: &UniqueWeight, a: &mut LockAttempt) {
        if a.remembered {
            a.target.page_ref().contended_unique_weights.remove_task_id(unique_weight);
        }
    }
    */

    fn reset_lock(&mut self, attempt: &mut LockAttempt, after_execution: bool) -> bool {
        match attempt.status {
            LockStatus::Succeded => {
                self.unlock(attempt)
            },
            LockStatus::Provisional => {
                if after_execution {
                    self.unlock(attempt)
                } else {
                    self.cancel(attempt);
                    false
                }
            }
            LockStatus::Failed => {
                false // do nothing
            }
        }
    }

    #[inline(never)]
    fn unlock(&mut self, attempt: &mut LockAttempt) -> bool {
        //debug_assert!(attempt.is_success());

        let mut newly_uncontended = false;
        let mut still_queued = false;

        let mut page = attempt.target.page_mut();

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
            if page.contended_unique_weights.has_more() {
                still_queued = true;
            }
        }

        still_queued
    }

    #[inline(never)]
    fn cancel(&mut self, attempt: &mut LockAttempt) {
        let mut page = attempt.target.page_mut();

        match page.next_usage {
            Usage::Unused => {
                unreachable!();
            },
            // support multiple readonly locks!
            Usage::Readonly(_) | Usage::Writable => {
                page.next_usage = Usage::Unused;
            },
        }
    }

    pub fn preloader(&self) -> Preloader {
        Preloader{book: std::sync::Arc::clone(&self.book)}
    }
}

pub struct Preloader {
    book: AddressMap,
}

impl Preloader {
    pub fn load(&self, address: Pubkey) -> PageRc {
        match self.book.entry(address) {
            AddressMapEntry::Vacant(book_entry) => {
                let page = PageRc(MyRcInner::new(Page::new(Usage::unused())));
                let cloned = PageRc::clone(&page);
                book_entry.insert(page);
                cloned
            }
            AddressMapEntry::Occupied(mut book_entry) => {
                let page = book_entry.get();
                PageRc::clone(&page)
            }
        }
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
pub struct Task {
    unique_weight: UniqueWeight,
    pub tx: Box<(SanitizedTransaction, Vec<LockAttempt>)>, // actually should be Bundle
    pub contention_count: usize,
    pub uncontended: std::sync::atomic::AtomicUsize,
}

impl Task {
    pub fn new_for_queue(unique_weight: UniqueWeight, tx: Box<(SanitizedTransaction, Vec<LockAttempt>)>) -> std::sync::Arc<Self> {
        TaskInQueue::new(Self { unique_weight, tx, contention_count: 0, uncontended: Default::default() })
    }

    pub fn clone_for_test(&self) {
        Self {
            unique_weight: self.unique_weight,
            tx: self.tx.clone(),
            contention_count: Default:default()
            uncontended: Default::default(),
        }
    }

    pub fn currently_contended(&self) -> bool {
        self.uncontended.load(std::sync::atomic::Ordering::SeqCst) == 1
    }

    fn mark_as_contended(&self) {
        self.uncontended.store(1, std::sync::atomic::Ordering::SeqCst)
    }

    fn mark_as_uncontended(&self) {
        self.uncontended.store(2, std::sync::atomic::Ordering::SeqCst)
    }
}

// RunnableQueue, ContendedQueue?
#[derive(Default, Debug, Clone)]
pub struct TaskQueue {
    tasks: std::collections::BTreeMap<UniqueWeight, TaskInQueue>,
    //tasks: im::OrdMap<UniqueWeight, TaskInQueue>,
    //tasks: im::HashMap<UniqueWeight, TaskInQueue>,
    //tasks: std::sync::Arc<dashmap::DashMap<UniqueWeight, TaskInQueue>>,
}

pub type TaskInQueue = std::sync::Arc<Task>;
//type TaskQueueEntry<'a> = im::ordmap::Entry<'a, UniqueWeight, TaskInQueue>;
//type TaskQueueOccupiedEntry<'a> = im::ordmap::OccupiedEntry<'a, UniqueWeight, TaskInQueue>;
//type TaskQueueEntry<'a> = im::hashmap::Entry<'a, UniqueWeight, TaskInQueue, std::collections::hash_map::RandomState>;
//type TaskQueueOccupiedEntry<'a> = im::hashmap::OccupiedEntry<'a, UniqueWeight, TaskInQueue, std::collections::hash_map::RandomState>;
//type TaskQueueEntry<'a> = dashmap::mapref::entry::Entry<'a, UniqueWeight, TaskInQueue>;
//type TaskQueueOccupiedEntry<'a> = dashmap::mapref::entry::OccupiedEntry<'a, UniqueWeight, TaskInQueue, std::collections::hash_map::RandomState>;
type TaskQueueEntry<'a> = std::collections::btree_map::Entry<'a, UniqueWeight, TaskInQueue>;
type TaskQueueOccupiedEntry<'a> = std::collections::btree_map::OccupiedEntry<'a, UniqueWeight, TaskInQueue>;

impl TaskQueue {
    #[inline(never)]
    fn add_to_schedule(&mut self, unique_weight: UniqueWeight, task: TaskInQueue) {
        //trace!("TaskQueue::add(): {:?}", unique_weight);
        let pre_existed = self.tasks.insert(unique_weight, task);
        debug_assert!(pre_existed.is_none()); //, "identical shouldn't exist: {:?}", unique_weight);
    }

    #[inline(never)]
    fn entry_to_execute(
        &mut self,
        unique_weight: UniqueWeight,
    ) -> Option<TaskQueueOccupiedEntry<'_>> {
        let queue_entry = self.tasks.entry(unique_weight);
        match queue_entry {
            TaskQueueEntry::Occupied(queue_entry) => Some(queue_entry),
            TaskQueueEntry::Vacant(_queue_entry) => None,
        }
    }

    #[inline(never)]
    pub fn has_contended_task(&self, unique_weight: &UniqueWeight) -> bool {
        let maybe_task = self.tasks.get(unique_weight);
        match maybe_task {
            Some(task) => task.currently_contended(),
            None => false,
        }
    }

    #[inline(never)]
    fn heaviest_entry_to_execute(
        &mut self,
    ) -> Option<TaskQueueOccupiedEntry<'_>> {
        //panic!()
        self.tasks.last_entry()
        //let k = self.tasks.get_max().map(|(k, _v)| *k);
        //panic!();
        //let k = self.tasks.iter().next().map(|(k, _v)| *k);
        //let k = self.tasks.iter().next().map(|r| *r.key());

        //k.map(|k| self.entry_to_execute(k).unwrap())
    }

    fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

#[inline(never)]
fn attempt_lock_for_execution<'a>(
    from_runnable: bool,
    prefer_immediate: bool,
    address_book: &mut AddressBook,
    unique_weight: &UniqueWeight,
    message_hash: &'a Hash,
    placeholder_attempts: &mut Vec<LockAttempt>,
) -> (usize, usize) {
    // no short-cuircuit; we at least all need to add to the contended queue
    let mut unlockable_count = 0;
    let mut provisional_count = 0;

    for attempt in placeholder_attempts.iter_mut() {
        AddressBook::attempt_lock_address(from_runnable, prefer_immediate, unique_weight, attempt);
        match attempt.status {
            LockStatus::Succeded => {},
            LockStatus::Failed => {
                unlockable_count += 1;
            },
            LockStatus::Provisional => {
                provisional_count += 1;
            },
        }
    }

    (unlockable_count, provisional_count)
}

type PreprocessedTransaction = (SanitizedTransaction, Vec<LockAttempt>);
// multiplexed to reduce the futex syscal per tx down to minimum and to make the schduler to
// adaptive relative load between sigverify stage and execution substage
// switched from crossbeam_channel::select! due to observed poor performance
pub enum Multiplexed {
    FromPrevious((Weight, TaskInQueue)),
    FromPreviousBatched(Vec<Vec<Box<PreprocessedTransaction>>>),
}

pub fn get_transaction_priority_details(tx: &SanitizedTransaction) -> u64 {
        use solana_program_runtime::compute_budget::ComputeBudget;
        let mut compute_budget = ComputeBudget::default();
        compute_budget
            .process_instructions(
                tx.message().program_instructions_iter(),
                true, // use default units per instruction
                true, // don't reject txs that use set compute unit price ix
            )
            .map(|d| d.get_priority()).unwrap_or_default()
}

pub struct ScheduleStage {}

impl ScheduleStage {
    fn push_to_runnable_queue(
        (weight, task): (Weight, TaskInQueue),
        runnable_queue: &mut TaskQueue,
        unique_key: &mut u64,
    ) {
        // manage randomness properly for future scheduling determinism
        //let mut rng = rand::thread_rng();

        //let ix = 23;
        //let tx = bank
        //    .verify_transaction(
        //        tx,
        //        solana_sdk::transaction::TransactionVerificationMode::FullVerification,
        //    )
        //    .unwrap();
        //tx.foo();
        //let unique_weight = (weight << 32) | (*unique_key & 0x0000_0000_ffff_ffff);
        let unique_weight = weight;

        runnable_queue.add_to_schedule(
            /*
            UniqueWeight {
                weight,
                //unique_key: solana_sdk::hash::new_rand(&mut rng),
                unique_key: *unique_key,
            },
            */
            unique_weight,
            task,
        );
        *unique_key -= 1;
    }

    /*
    #[inline(never)]
    fn get_newly_u_u_w<'a>(
        address: &'a Pubkey,
        address_book: &'a AddressBook,
    ) -> &'a std::collections::BTreeSet<UniqueWeight> {
        &address_book
            .book
            .get(address)
            .unwrap()
            .0
            .contended_unique_weights
    }
    */

    #[inline(never)]
    fn get_heaviest_from_contended<'a>(address_book: &'a mut AddressBook) -> Option<std::collections::btree_map::OccupiedEntry<'a, UniqueWeight, TaskInQueue>> {
        address_book.uncontended_task_ids.last_entry()
    }

    #[inline(never)]
    fn select_next_task<'a>(
        runnable_queue: &'a mut TaskQueue,
        address_book: &mut AddressBook,
    ) -> Option<(
        bool,
        TaskInQueue,
    )> {
        match (
            runnable_queue.heaviest_entry_to_execute(),
            Self::get_heaviest_from_contended(address_book),
        ) {
            (Some(heaviest_runnable_entry), None) => {
                trace!("select: runnable only");
                let t = heaviest_runnable_entry.remove();
                return Some((true, t))
            }
            (None, Some(weight_from_contended)) => {
                trace!("select: contended only");
                let uw = *weight_from_contended.key();
                let t = weight_from_contended.remove();

                return Some(( false, t))
            },
            (Some(heaviest_runnable_entry), Some(weight_from_contended)) => {
                let weight_from_runnable = heaviest_runnable_entry.key();
                let uw = weight_from_contended.key();

                if true || weight_from_runnable > uw {
                    trace!("select: runnable > contended");
                    let t = heaviest_runnable_entry.remove();
                    return Some((true, t))
                } else if false && uw > weight_from_runnable {
                    panic!();
                    /*
                    trace!("select: contended > runnnable");
                    let uw = *uw;
                    weight_from_contended.remove();
                    if let Some (queue_entry) = contended_queue.entry_to_execute(uw) {
                        return Some(( None, queue_entry))
                    } else {
                        unreachable!();
                    }
                    */
                } else {
                    unreachable!(
                        "identical unique weights shouldn't exist in both runnable and contended"
                    )
                }
            }
            (None, None) => {
                trace!("select: none");
                return None
            }
        }
    }

    #[inline(never)]
    fn pop_from_queue_then_lock(
        runnable_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
        contended_count: &mut usize,
        prefer_immediate: bool,
    ) -> Option<(UniqueWeight, TaskInQueue)> {
        if let Some(a) = address_book.fulfilled_provisional_task_ids.pop_last() {
            trace!("expediate pop from provisional queue [rest: {}]", address_book.fulfilled_provisional_task_ids.len());
            return Some((a.0, a.1));
        }

        trace!("pop begin");
        loop {
        if let Some((from_runnable, mut arc_next_task)) = Self::select_next_task(runnable_queue, address_book) {
            trace!("pop loop iteration");
            let next_task = unsafe { TaskInQueue::get_mut_unchecked(&mut arc_next_task) };
            let unique_weight = next_task.unique_weight;
            let message_hash = next_task.tx.0.message_hash();

            // plumb message_hash into StatusCache or implmenent our own for duplicate tx
            // detection?

            let (unlockable_count, provisional_count) = attempt_lock_for_execution(
                from_runnable,
                prefer_immediate,
                address_book,
                &unique_weight,
                &message_hash,
                &mut next_task.tx.1,
            );

            if unlockable_count > 0 {
                //trace!("reset_lock_for_failed_execution(): {:?} {}", (&unique_weight, from_runnable), next_task.tx.0.signature());
                Self::reset_lock_for_failed_execution(
                    address_book,
                    &unique_weight,
                    &mut next_task.tx.1,
                    from_runnable,
                );
                let lock_count = next_task.tx.1.len();
                next_task.contention_count += 1;

                if from_runnable {
                    trace!("move to contended due to lock failure [{}/{}/{}]", unlockable_count, provisional_count, lock_count);
                    next_task.mark_as_contended();
                    *contended_count += 1;
                    // maybe run lightweight prune logic on contended_queue here.
                } else {
                    trace!("relock failed [{}/{}/{}]; remains in contended: {:?} contention: {}", unlockable_count, provisional_count, lock_count, &unique_weight, next_task.contention_count);
                    //address_book.uncontended_task_ids.clear();
                }

                if from_runnable {
                    continue;
                } else {
                    return None;
                }
            } else if provisional_count > 0 {
                assert!(!from_runnable);
                let lock_count = next_task.tx.1.len();
                trace!("provisional exec: [{}/{}]", provisional_count, lock_count);
                Self::finalize_lock_for_provisional_execution(
                    address_book,
                    &unique_weight,
                    &mut next_task.tx.1,
                    provisional_count,
                );
                drop(next_task);
                address_book.provisioning_trackers.insert(unique_weight, (ProvisioningTracker::new(provisional_count), arc_next_task));

                return None;
                continue;
            }

            trace!("successful lock: (from_runnable: {}) after {} contentions", from_runnable, next_task.contention_count);
            Self::finalize_lock_before_execution(
                address_book,
                &unique_weight,
                &mut next_task.tx.1,
            );
            return Some((unique_weight, arc_next_task));
        } else {
            break;
        }
        }

        None
    }

    #[inline(never)]
    // naming: relock_before_execution() / update_address_book() / update_uncontended_addresses()?
    fn finalize_lock_before_execution(
        address_book: &mut AddressBook,
        unique_weight: &UniqueWeight,
        lock_attempts: &mut Vec<LockAttempt>,
    ) {
        for mut l in lock_attempts {
            // ensure to remove remaining refs of this unique_weight
            //AddressBook::forget_address_contention(&unique_weight, &mut l);
        }
    }

    #[inline(never)]
    fn finalize_lock_for_provisional_execution(
        address_book: &mut AddressBook,
        unique_weight: &UniqueWeight,
        lock_attempts: &mut Vec<LockAttempt>,
        provisional_count: usize,
    ) {
        for mut l in lock_attempts {
            //AddressBook::forget_address_contention(&unique_weight, &mut l);
            match l.status {
                LockStatus::Provisional => {
                    l.target.page_mut().provisional_task_ids.insert(*unique_weight);
                }
                LockStatus::Succeded => {
                    // do nothing
                }
                LockStatus::Failed => {
                    unreachable!();
                }
            }
        }
        trace!("provisioning_trackers: {}", address_book.provisioning_trackers.len());
    }

    #[inline(never)]
    fn reset_lock_for_failed_execution(
        address_book: &mut AddressBook,
        unique_weight: &UniqueWeight,
        lock_attempts: &mut Vec<LockAttempt>,
        from_runnable: bool,
    ) {
        for l in lock_attempts {
            if from_runnable {
                //AddressBook::remember_address_contention(&mut l.target.page_mut(), unique_weight);
                //l.remembered = true;
            }
            address_book.reset_lock(l, false);
            //if let Some(uw) = l.target.page().contended_unique_weights.last() {
            //    address_book.uncontended_task_ids.remove(uw);
            //}

            // todo: mem::forget and panic in LockAttempt::drop()
        }
    }

    #[inline(never)]
    fn unlock_after_execution(address_book: &mut AddressBook, lock_attempts: &mut Vec<LockAttempt>) {
        for mut l in lock_attempts.into_iter() {
            let newly_uncontended_while_queued = address_book.reset_lock(&mut l, true);

            let page = l.target.page_mut();
            if newly_uncontended_while_queued && page.next_usage == Usage::Unused {
                let maybe_task = l.heaviest_uncontended.load_full();
                if let Some(task) = maybe_task {
                    if task.currently_contended() {
                        address_book.uncontended_task_ids.insert(task.unique_weight, task);
                    }
                }
            }
            if page.current_usage == Usage::Unused && page.next_usage != Usage::Unused {
                page.switch_to_next_usage();
                for task_id in std::mem::take(&mut page.provisional_task_ids).into_iter() {
                    match address_book.provisioning_trackers.entry(task_id) {
                        std::collections::hash_map::Entry::Occupied(mut tracker_entry) => {
                            let (tracker, task) = tracker_entry.get_mut();
                            tracker.tick();
                            if tracker.is_fulfilled() {
                                trace!("provisioning tracker tick: {} => {} (!)", tracker.prev_count(), tracker.count());
                                let (tracker, task) = tracker_entry.remove();
                                address_book.fulfilled_provisional_task_ids.insert(task_id, task);
                            } else {
                                trace!("provisioning tracker tick: {} => {}", tracker.prev_count(), tracker.count());
                            }
                        },
                        std::collections::hash_map::Entry::Vacant(_) => {
                            unreachable!();
                        }
                    }
                }
            }

            // todo: mem::forget and panic in LockAttempt::drop()
        }
    }

    #[inline(never)]
    fn prepare_scheduled_execution(
        address_book: &mut AddressBook,
        unique_weight: UniqueWeight,
        task: TaskInQueue,
        contended_count: &mut usize,
    ) -> Box<ExecutionEnvironment> {
        let mut rng = rand::thread_rng();
        // load account now from AccountsDb
        task.mark_as_uncontended();
        *contended_count -= 1;

        Box::new(ExecutionEnvironment {
            task,
            unique_weight,
            cu: rng.gen_range(3, 1000),
        })
    }

    #[inline(never)]
    fn commit_result(ee: &mut ExecutionEnvironment, address_book: &mut AddressBook) {
        // do par()-ly?
        let mut task = unsafe { TaskInQueue::get_mut_unchecked(&mut ee.task) };
        Self::unlock_after_execution(address_book, &mut task.tx.1);
        // block-wide qos validation will be done here
        // if error risen..:
        //   don't commit the tx for banking and potentially finish scheduling at block max cu
        //   limit
        //   mark the block as dead for replaying

        // par()-ly clone updated Accounts into address book
    }

    #[inline(never)]
    fn schedule_next_execution(
        runnable_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
        contended_count: &mut usize,
        prefer_immediate: bool,
    ) -> Option<Box<ExecutionEnvironment>> {
        let maybe_ee =
            Self::pop_from_queue_then_lock(runnable_queue, address_book, contended_count, prefer_immediate)
                .map(|(uw, t)| Self::prepare_scheduled_execution(address_book, uw, t, contended_count));
        maybe_ee
    }

    #[inline(never)]
    fn register_runnable_task(
        weighted_tx: (Weight, TaskInQueue),
        runnable_queue: &mut TaskQueue,
        unique_key: &mut u64,
    ) {
        Self::push_to_runnable_queue(weighted_tx, runnable_queue, unique_key)
    }

    pub fn run(
        max_executing_queue_count: usize,
        runnable_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
        from: &crossbeam_channel::Receiver<Multiplexed>,
        from_exec: &crossbeam_channel::Receiver<Box<ExecutionEnvironment>>,
        to_execute_substage: &crossbeam_channel::Sender<Box<ExecutionEnvironment>>,
        maybe_to_next_stage: Option<&crossbeam_channel::Sender<Box<ExecutionEnvironment>>>, // assume nonblocking
    ) {
        let mut executing_queue_count = 0;
        let mut current_unique_key = u64::max_value();
        let mut contended_count = 0;
        let (ee_sender, ee_receiver) = crossbeam_channel::unbounded::<Box<ExecutionEnvironment>>();

        let (to_next_stage, maybe_jon_handle) = if let Some(to_next_stage) = maybe_to_next_stage {
            (to_next_stage, None)
        } else {
            let h = std::thread::Builder::new().name("sol-reaper".to_string()).spawn(move || {
                while let a = ee_receiver.recv().unwrap() {
                    drop(a);
                }
            }).unwrap();

            (&ee_sender, Some(h))
        };

        loop {
            trace!("schedule_once (from: {}, to: {}, runnnable: {}, contended: {}, (immediate+provisional)/max: ({}+{})/{}) active from contended: {}!", from.len(), to_execute_substage.len(), runnable_queue.task_count(), contended_count, executing_queue_count, address_book.provisioning_trackers.len(), max_executing_queue_count, address_book.uncontended_task_ids.len());

            crossbeam_channel::select! {
               recv(from) -> maybe_from => {
                   let i = maybe_from.unwrap();
                    match i {
                        Multiplexed::FromPrevious(weighted_tx) => {
                            trace!("recv from previous");

                            while false && from_exec.len() > 0 {
                                let mut processed_execution_environment = from_exec.recv().unwrap();
                                trace!("recv from execute: {:?}", processed_execution_environment.unique_weight);
                                executing_queue_count -= 1;

                                Self::commit_result(&mut processed_execution_environment, address_book);
                                // async-ly propagate the result to rpc subsystems
                                to_next_stage.send(processed_execution_environment).unwrap();
                            }

                            Self::register_runnable_task(weighted_tx, runnable_queue, &mut current_unique_key);

                            while from.len() > 0 && from_exec.len() == 0 {
                               let i = from.recv().unwrap();
                                match i {
                                    Multiplexed::FromPrevious(weighted_tx) => {
                                        trace!("recv from previous (after starvation)");
                                        Self::register_runnable_task(weighted_tx, runnable_queue, &mut current_unique_key);
                                    }
                                    Multiplexed::FromPreviousBatched(vvv) => {
                                        unreachable!();
                                    }
                                }
                            }
                        }
                        Multiplexed::FromPreviousBatched(vvv) => {
                            unreachable!();
                        }
                    }
               }
               recv(from_exec) -> maybe_from_exec => {
                   let mut processed_execution_environment = maybe_from_exec.unwrap();
                    loop {
                        trace!("recv from execute: {:?}", processed_execution_environment.unique_weight);
                        executing_queue_count -= 1;

                        Self::commit_result(&mut processed_execution_environment, address_book);
                        // async-ly propagate the result to rpc subsystems
                        to_next_stage.send(processed_execution_environment).unwrap();
                        if false && from_exec.len() > 0 {
                            processed_execution_environment = from_exec.recv().unwrap();
                        } else {
                            break;
                        }
                    }
               }
            }

            loop {
                /* if !address_book.uncontended_task_ids.is_empty() {
                    trace!("prefer emptying n_u_a");
                } else */ if (executing_queue_count + address_book.provisioning_trackers.len()) >= max_executing_queue_count {
                    trace!("skip scheduling; outgoing queue full");
                    while from.len() > 0 {
                       let i = from.recv().unwrap();
                        match i {
                            Multiplexed::FromPrevious(weighted_tx) => {
                                trace!("recv from previous (after starvation)");
                                Self::register_runnable_task(weighted_tx, runnable_queue, &mut current_unique_key);
                            }
                            Multiplexed::FromPreviousBatched(vvv) => {
                                unreachable!();
                            }
                        }
                    }
                    break;
                }

                let prefer_immediate = address_book.provisioning_trackers.len()/4 > executing_queue_count;
                let maybe_ee =
                    Self::schedule_next_execution(runnable_queue, address_book, &mut contended_count, prefer_immediate);

                if let Some(ee) = maybe_ee {
                    trace!("send to execute");
                    executing_queue_count += 1;

                    to_execute_substage.send(ee).unwrap();
                } else {
                    trace!("incoming queue starved");
                    while from.len() > 0 {
                       let i = from.recv().unwrap();
                        match i {
                            Multiplexed::FromPrevious(weighted_tx) => {
                                trace!("recv from previous (after starvation)");
                                Self::register_runnable_task(weighted_tx, runnable_queue, &mut current_unique_key);
                            }
                            Multiplexed::FromPreviousBatched(vvv) => {
                                unreachable!();
                            }
                        }
                    }
                    break;
                }
            }
        }
    }
}

struct ExecuteStage {
    //bank: Bank,
}

impl ExecuteStage {}
