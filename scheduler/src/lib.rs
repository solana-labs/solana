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

type MyRcInner<T> = std::sync::Arc<T>;

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct PageRc(ByAddress<MyRcInner<Page>>);


#[derive(Debug)]
pub struct ExecutionEnvironment {
    lock_attempts: Vec<LockAttempt>,
    //accounts: Vec<i8>,
    pub cu: usize,
    pub task: Task,
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

#[derive(Clone, Debug)]
enum AddressLookup {
    Before(Pubkey),
    After(PageRc),
}

impl AddressLookup {
    fn page_rc(&mut self) -> &PageRc {
        match self {
            AddressLookup::Before(_) => unreachable!(),
            AddressLookup::After(page) => page,
        }
    }

    fn take_page_rc(mut self) -> PageRc {
        match self {
            AddressLookup::Before(_) => unreachable!(),
            AddressLookup::After(page) => page,
        }
    }

    fn page(&mut self) -> &mut Page {
        match self {
            AddressLookup::Before(_) => unreachable!(),
            AddressLookup::After(page) => unsafe { MyRcInner::get_mut_unchecked(&mut page.0) },
        }
    }
}

impl PageRc {
    fn page(&mut self) -> &mut Page {
        unsafe { MyRcInner::get_mut_unchecked(&mut self.0) }
    }
}

#[derive(Clone, Debug)]
enum LockStatus {
    Succeded,
    Guaranteed, 
    Failed,
}

#[derive(Clone, Debug)]
pub struct LockAttempt {
    target: PageRc,
    status: LockStatus,
    requested_usage: RequestedUsage,
    remembered: bool,
}

impl LockAttempt {
    pub fn new(target: PageRc, requested_usage: RequestedUsage) -> Self {
        Self {
            target,
            status: LockStatus::Succeded,
            requested_usage,
            remembered: false,
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

#[derive(Debug)]
struct Page {
    current_usage: Usage,
    next_usage: Usage,
    contended_unique_weights: std::collections::BTreeSet<UniqueWeight>,
    guaranteed_task_ids: WeightedTaskIds,
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
            guaranteed_task_ids: Default::default(),
        }
    }

    fn switch_to_next_usage(&mut self) {
        self.current_usage = self.next_usage;
    }
}

//type AddressMap = std::collections::HashMap<Pubkey, PageRc>;
type AddressMap = std::sync::Arc<dashmap::DashMap<Pubkey, PageRc>>;
use by_address::ByAddress;
type TaskId = UniqueWeight;
type WeightedTaskIds = std::collections::BTreeMap<TaskId, ()>;
//type AddressMapEntry<'a, K, V> = std::collections::hash_map::Entry<'a, K, V>;
type AddressMapEntry<'a> = dashmap::mapref::entry::Entry<'a, Pubkey, PageRc>;

// needs ttl mechanism and prune
#[derive(Default)]
pub struct AddressBook {
    book: AddressMap,
    uncontended_task_ids: WeightedTaskIds,
    runnable_guaranteed_task_ids: WeightedTaskIds,
    guaranteed_lock_counts: std::collections::HashMap<UniqueWeight, usize>, 
}

impl AddressBook {
    #[inline(never)]
    fn attempt_lock_address(
        from_runnable: bool,
        unique_weight: &UniqueWeight,
        attempt: &mut LockAttempt,
    ) {
        let LockAttempt {target, requested_usage, status, remembered} = attempt;

                let mut page = unsafe { MyRcInner::get_mut_unchecked(&mut target.0) };

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
                            if from_runnable {
                                //Self::remember_address_contention(&mut page, unique_weight);
                                //*remembered = true;
                                *status = LockStatus::Failed;
                            } else {
                                match page.next_usage {
                                    Usage::Unused => {
                                        *status = LockStatus::Guaranteed;
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
                        if from_runnable {
                            //Self::remember_address_contention(&mut page, unique_weight);
                            //*remembered = true;
                            *status = LockStatus::Failed;
                        } else {
                            match page.next_usage {
                                Usage::Unused => {
                                    *status = LockStatus::Guaranteed;
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

    #[inline(never)]
    fn remember_address_contention(page: &mut Page, unique_weight: &UniqueWeight) {
        page.contended_unique_weights.insert(*unique_weight);
    }

    #[inline(never)]
    fn forget_address_contention(unique_weight: &UniqueWeight, a: &mut LockAttempt) {
        if a.remembered {
            a.target.page().contended_unique_weights.remove(unique_weight);
        }
    }

    fn reset_lock(&mut self, attempt: &mut LockAttempt, after_execution: bool) -> bool {
        match attempt.status {
            LockStatus::Succeded => {
                self.unlock(attempt)
            },
            LockStatus::Guaranteed => {
                self.cancel(attempt);
                if after_execution {
                    self.unlock(attempt)
                } else {
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

        let mut page = attempt.target.page();

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
            if !page.contended_unique_weights.is_empty() {
                still_queued = true;
            }
        }

        still_queued
    }

    #[inline(never)]
    fn cancel(&mut self, attempt: &mut LockAttempt) {
        let mut page = attempt.target.page();

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
                let page = PageRc(ByAddress(MyRcInner::new(Page::new(Usage::unused()))));
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
/*
pub type Weight = usize;
pub type UniqueWeight = usize;
*/

struct Bundle {
    // what about bundle1{tx1a, tx2} and bundle2{tx1b, tx2}?
}

#[derive(Debug)]
pub struct Task {
    pub tx: Box<(SanitizedTransaction, Vec<LockAttempt>)>, // actually should be Bundle
    pub contention_count: usize,
}

// RunnableQueue, ContendedQueue?
#[derive(Default)]
pub struct TaskQueue {
    tasks: std::collections::BTreeMap<UniqueWeight, Task>,
}

impl TaskQueue {
    #[inline(never)]
    fn add_to_schedule(&mut self, unique_weight: UniqueWeight, task: Task) {
        //trace!("TaskQueue::add(): {:?}", unique_weight);
        let pre_existed = self.tasks.insert(unique_weight, task);
        debug_assert!(pre_existed.is_none()); //, "identical shouldn't exist: {:?}", unique_weight);
    }

    #[inline(never)]
    fn entry_to_execute(
        &mut self,
        unique_weight: UniqueWeight,
    ) -> std::collections::btree_map::OccupiedEntry<'_, UniqueWeight, Task> {
        use std::collections::btree_map::Entry;

        let queue_entry = self.tasks.entry(unique_weight);
        match queue_entry {
            Entry::Occupied(queue_entry) => queue_entry,
            Entry::Vacant(_queue_entry) => unreachable!(),
        }
    }

    #[inline(never)]
    fn heaviest_entry_to_execute(
        &mut self,
    ) -> Option<std::collections::btree_map::OccupiedEntry<'_, UniqueWeight, Task>> {
        self.tasks.last_entry()
    }

    fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

#[inline(never)]
fn attempt_lock_for_execution<'a>(
    from_runnable: bool,
    address_book: &mut AddressBook,
    unique_weight: &UniqueWeight,
    message_hash: &'a Hash,
    mut placeholder_attempts: Vec<LockAttempt>,
) -> (usize, usize, Vec<LockAttempt>) {
    // no short-cuircuit; we at least all need to add to the contended queue
    let mut unlockable_count = 0;
    let mut guaranteed_count = 0;

    for attempt in placeholder_attempts.iter_mut() {
        AddressBook::attempt_lock_address(from_runnable, unique_weight, attempt);
        match attempt.status {
            LockStatus::Succeded => {},
            LockStatus::Failed => {
                unlockable_count += 1;
            },
            LockStatus::Guaranteed => {
                guaranteed_count += 1;
            },
        }
    }

    (unlockable_count, guaranteed_count, placeholder_attempts)
}

type PreprocessedTransaction = (SanitizedTransaction, Vec<LockAttempt>, u64);
// multiplexed to reduce the futex syscal per tx down to minimum and to make the schduler to
// adaptive relative load between sigverify stage and execution substage
// switched from crossbeam_channel::select! due to observed poor performance
pub enum Multiplexed {
    FromPrevious((Weight, Box<PreprocessedTransaction>)),
    FromPreviousBatched(Vec<Vec<Box<PreprocessedTransaction>>>),
    FromExecute(Box<ExecutionEnvironment>),
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
    fn push_to_queue(
        (weight, tx): (Weight, Box<(SanitizedTransaction, Vec<LockAttempt>, u64)>),
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

        runnable_queue.add_to_schedule(
            UniqueWeight {
                weight,
                //unique_key: solana_sdk::hash::new_rand(&mut rng),
                unique_key: *unique_key,
            },
            Task { tx, contention_count: 0 },
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
    fn get_heaviest_from_contended<'a>(address_book: &'a mut AddressBook) -> Option<std::collections::btree_map::OccupiedEntry<'a, UniqueWeight, ()>> {
        trace!("n_u_a len(): {}", address_book.uncontended_task_ids.len());
        address_book.uncontended_task_ids.last_entry()
    }

    #[inline(never)]
    fn select_next_task<'a>(
        runnable_queue: &'a mut TaskQueue,
        contended_queue: &'a mut TaskQueue,
        address_book: &mut AddressBook,
    ) -> Option<(
        Option<&'a mut TaskQueue>,
        std::collections::btree_map::OccupiedEntry<'a, UniqueWeight, Task>,
    )> {
        match (
            runnable_queue.heaviest_entry_to_execute(),
            Self::get_heaviest_from_contended(address_book),
        ) {
            (Some(heaviest_runnable_entry), None) => {
                trace!("select: runnable only");
                Some((Some(contended_queue), heaviest_runnable_entry))
            }
            (None, Some(weight_from_contended)) => {
                trace!("select: contended only");
                let uw = *weight_from_contended.key();
                weight_from_contended.remove();

                Some(( None, contended_queue.entry_to_execute(uw)))
            },
            (Some(heaviest_runnable_entry), Some(weight_from_contended)) => {
                let weight_from_runnable = heaviest_runnable_entry.key();
                let uw = weight_from_contended.key();

                if weight_from_runnable > uw {
                    trace!("select: runnable > contended");
                    Some((Some(contended_queue), heaviest_runnable_entry))
                } else if uw > weight_from_runnable {
                    trace!("select: contended > runnnable");
                    let uw = *uw;
                    weight_from_contended.remove();
                    Some((
                        None,
                        contended_queue.entry_to_execute(uw),
                    ))
                } else {
                    unreachable!(
                        "identical unique weights shouldn't exist in both runnable and contended"
                    )
                }
            }
            (None, None) => {
                trace!("select: none");
                None
            }
        }
    }

    #[inline(never)]
    fn pop_from_queue_then_lock(
        runnable_queue: &mut TaskQueue,
        contended_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
    ) -> Option<(UniqueWeight, Task, Vec<LockAttempt>)> {
        if let Some(a) = address_book.runnable_guaranteed_task_ids.pop_last() {
            trace!("expediate pop from guaranteed queue [rest: {}]", address_book.runnable_guaranteed_task_ids.len());
            let queue_entry = contended_queue.entry_to_execute(a.0);
            let mut task = queue_entry.remove();
            let ll = std::mem::take(&mut task.tx.1);
            return Some((a.0, task, ll));
        }

        trace!("pop begin");
        loop {
        if let Some((reborrowed_contended_queue, mut queue_entry)) = Self::select_next_task(runnable_queue, contended_queue, address_book) {
            trace!("pop loop iteration");
            let from_runnable = reborrowed_contended_queue.is_some();
            let unique_weight = *queue_entry.key();
            let next_task = queue_entry.get_mut();
            let message_hash = next_task.tx.0.message_hash();
            let placeholder_lock_attempts = std::mem::take(&mut next_task.tx.1);

            // plumb message_hash into StatusCache or implmenent our own for duplicate tx
            // detection?

            let (unlockable_count, guaranteed_count, mut populated_lock_attempts) = attempt_lock_for_execution(
                from_runnable,
                address_book,
                &unique_weight,
                &message_hash,
                placeholder_lock_attempts,
            );

            if unlockable_count > 0 {
                //trace!("reset_lock_for_failed_execution(): {:?} {}", (&unique_weight, from_runnable), next_task.tx.0.signature());
                Self::reset_lock_for_failed_execution(
                    address_book,
                    &unique_weight,
                    &mut populated_lock_attempts,
                    from_runnable,
                );
                let lock_count = populated_lock_attempts.len();
                std::mem::swap(&mut next_task.tx.1, &mut populated_lock_attempts);
                next_task.contention_count += 1;

                if from_runnable {
                    trace!("move to contended due to lock failure [{}/{}/{}]", unlockable_count, guaranteed_count, lock_count);
                    reborrowed_contended_queue
                        .unwrap()
                        .add_to_schedule(*queue_entry.key(), queue_entry.remove());
                    // maybe run lightweight prune logic on contended_queue here.
                } else {
                    trace!("relock failed [{}/{}/{}]; remains in contended: {:?} contention: {}", unlockable_count, guaranteed_count, lock_count, &unique_weight, next_task.contention_count);
                }

                continue;
            } else if guaranteed_count > 0 {
                assert!(!from_runnable);
                let lock_count = populated_lock_attempts.len();
                trace!("guaranteed exec: [{}/{}]", guaranteed_count, lock_count);
                Self::finalize_lock_for_guaranteed_execution(
                    address_book,
                    &unique_weight,
                    &mut populated_lock_attempts,
                    guaranteed_count,
                );
                std::mem::swap(&mut next_task.tx.1, &mut populated_lock_attempts);

                continue;
            }

            trace!("successful lock: (from_runnable: {}) after {} contentions", from_runnable, next_task.contention_count);
            Self::finalize_lock_before_execution(
                address_book,
                &unique_weight,
                &mut populated_lock_attempts,
            );
            let task = queue_entry.remove();
            return Some((unique_weight, task, populated_lock_attempts));
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
            AddressBook::forget_address_contention(&unique_weight, &mut l);
        }
    }

    #[inline(never)]
    fn finalize_lock_for_guaranteed_execution(
        address_book: &mut AddressBook,
        unique_weight: &UniqueWeight,
        lock_attempts: &mut Vec<LockAttempt>,
        guaranteed_count: usize,
    ) {
        for mut l in lock_attempts {
            AddressBook::forget_address_contention(&unique_weight, &mut l);
            match l.status {
                LockStatus::Guaranteed => {
                    l.target.page().guaranteed_task_ids.insert(*unique_weight, ());
                }
                LockStatus::Succeded => {
                    // do nothing
                }
                LockStatus::Failed => {
                    unreachable!();
                }
            }
        }
        address_book.guaranteed_lock_counts.insert(*unique_weight, guaranteed_count);
        trace!("guaranteed_lock_counts: {}", address_book.guaranteed_lock_counts.len());
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
                AddressBook::remember_address_contention(&mut l.target.page(), unique_weight);
                l.remembered = true;
            }
            address_book.reset_lock(l, false);
            //if let Some(uw) = l.target.page().contended_unique_weights.last() {
            //    address_book.uncontended_task_ids.remove(uw);
            //}

            // todo: mem::forget and panic in LockAttempt::drop()
        }
    }

    #[inline(never)]
    fn unlock_after_execution(address_book: &mut AddressBook, lock_attempts: Vec<LockAttempt>) {
        for mut l in lock_attempts.into_iter() {
            let newly_uncontended_while_queued = address_book.reset_lock(&mut l, true);

            let page = l.target.page();
            if newly_uncontended_while_queued && page.next_usage == Usage::Unused {
                if let Some(uw) = page.contended_unique_weights.last() {
                    address_book.uncontended_task_ids.insert(*uw, ());
                }
            }
            if page.current_usage == Usage::Unused && page.next_usage != Usage::Unused {
                page.switch_to_next_usage();
                for task_id in std::mem::take(&mut page.guaranteed_task_ids).keys() {
                    match address_book.guaranteed_lock_counts.entry(*task_id) {
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            let count = entry.get_mut();
                            trace!("guaranteed lock decrease: {} => {}", *count, *count -1);
                            *count = count.checked_sub(1).unwrap();
                            if *count == 0 {
                                entry.remove();
                                address_book.runnable_guaranteed_task_ids.insert(*task_id, ());
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
        task: Task,
        lock_attempts: Vec<LockAttempt>,
    ) -> Box<ExecutionEnvironment> {
        let mut rng = rand::thread_rng();
        // load account now from AccountsDb

        Box::new(ExecutionEnvironment {
            lock_attempts,
            task,
            cu: rng.gen_range(3, 1000),
        })
    }

    #[inline(never)]
    fn commit_result(ee: &mut ExecutionEnvironment, address_book: &mut AddressBook) {
        let lock_attempts = std::mem::take(&mut ee.lock_attempts);
        // do par()-ly?
        Self::unlock_after_execution(address_book, lock_attempts);
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
        contended_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
    ) -> Option<Box<ExecutionEnvironment>> {
        let maybe_ee =
            Self::pop_from_queue_then_lock(runnable_queue, contended_queue, address_book)
                .map(|(uw, t, ll)| Self::prepare_scheduled_execution(address_book, uw, t, ll));
        maybe_ee
    }

    #[inline(never)]
    fn register_runnable_task(
        weighted_tx: (Weight, Box<(SanitizedTransaction, Vec<LockAttempt>, u64)>),
        runnable_queue: &mut TaskQueue,
        unique_key: &mut u64,
    ) {
        Self::push_to_queue(weighted_tx, runnable_queue, unique_key)
    }

    pub fn run(
        max_executing_queue_count: usize,
        runnable_queue: &mut TaskQueue,
        contended_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
        from: &crossbeam_channel::Receiver<Multiplexed>,
        to_execute_substage: &crossbeam_channel::Sender<Box<ExecutionEnvironment>>,
        to_next_stage: Option<&crossbeam_channel::Sender<Box<ExecutionEnvironment>>>, // assume nonblocking
    ) {
        let mut executing_queue_count = 0;
        let mut current_unique_key = u64::max_value();

        loop {
            trace!("schedule_once (runnnable: {}, contended: {}, guaranteed: {}, exec: {})!", runnable_queue.task_count(), contended_queue.task_count(), address_book.guaranteed_lock_counts.len(), executing_queue_count);

            let i = from.recv().unwrap();
            match i {
                Multiplexed::FromPrevious(weighted_tx) => {
                    trace!("recv from previous");

                    Self::register_runnable_task(weighted_tx, runnable_queue, &mut current_unique_key);
                }
                Multiplexed::FromPreviousBatched(vvv) => {
                    trace!("recv from previous");

                    for vv in vvv {
                        for v in vv {
                            Self::register_runnable_task((Weight { ix: v.2 }, v), runnable_queue, &mut current_unique_key);
                            if executing_queue_count < max_executing_queue_count {
                                let maybe_ee =
                                    Self::schedule_next_execution(runnable_queue, contended_queue, address_book);
                                if let Some(ee) = maybe_ee {
                                    trace!("batched: send to execute");
                                    executing_queue_count += 1;

                                    to_execute_substage.send(ee).unwrap();
                                }
                            } else {
                                    trace!("batched: outgoing queue full");
                            }
                        }
                    }
                }
                Multiplexed::FromExecute(mut processed_execution_environment) => {
                    trace!("recv from execute");
                    executing_queue_count -= 1;

                    Self::commit_result(&mut processed_execution_environment, address_book);
                    // async-ly propagate the result to rpc subsystems
                    if let Some(to_next_stage) = to_next_stage {
                        to_next_stage.send(processed_execution_environment).unwrap();
                    }
                }
            }

            loop {
                /*if !address_book.uncontended_task_ids.is_empty() {
                    trace!("prefer emptying n_u_a");
                } else */ if executing_queue_count >= max_executing_queue_count {
                    trace!("outgoing queue full");
                    break;
                }

                let maybe_ee =
                    Self::schedule_next_execution(runnable_queue, contended_queue, address_book);

                if let Some(ee) = maybe_ee {
                    trace!("send to execute");
                    executing_queue_count += 1;

                    to_execute_substage.send(ee).unwrap();
                } else {
                    trace!("incoming queue starved: n_u_a: {}", address_book.uncontended_task_ids.len());
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
