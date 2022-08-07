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

type MyRcInner<T> = std::sync::Arc<T>;
type MyRc<T> = ByAddress<MyRcInner<T>>;

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
enum LockAttemptStatus {
    BeforeLookup(Pubkey),
    AfterLookup(Pubkey, MyRc<Page>),
}

impl LockAttemptStatus {
    fn address2(&self) -> &Pubkey {
        match self {
            LockAttemptStatus::BeforeLookup(pubkey) => &pubkey,
            LockAttemptStatus::AfterLookup(pubkey, _) => &pubkey,
        }
    }

    fn page_rc(&mut self) -> &MyRc<Page> {
        match self {
            LockAttemptStatus::BeforeLookup(_) => unreachable!(),
            LockAttemptStatus::AfterLookup(_address, page) => page,
        }
    }

    fn page(&mut self) -> &mut Page {
        match self {
            LockAttemptStatus::BeforeLookup(_) => unreachable!(),
            LockAttemptStatus::AfterLookup(_address, page) => unsafe { MyRcInner::get_mut_unchecked(page) },
        }
    }
}

#[derive(Clone, Debug)]
pub struct LockAttempt {
    status: LockAttemptStatus,
    is_success: bool,
    requested_usage: RequestedUsage,
}

impl LockAttempt {
    fn is_success(&self) -> bool {
        self.is_success
    }

    fn is_failed(&self) -> bool {
        !self.is_success()
    }

    pub fn new(address: Pubkey, requested_usage: RequestedUsage) -> Self {
        Self {
            status: LockAttemptStatus::BeforeLookup(address),
            is_success: true,
            requested_usage,
        }
    }
}

type UsageCount = usize;
const SOLE_USE_COUNT: UsageCount = 1;

#[derive(Debug, PartialEq)]
enum CurrentUsage {
    Unused,
    // weight to abort running tx?
    // also sum all readonly weights to subvert to write lock with greater weight?
    Readonly(UsageCount),
    Writable,
}

impl CurrentUsage {
    fn renew(requested_usage: RequestedUsage) -> Self {
        match requested_usage {
            RequestedUsage::Readonly => CurrentUsage::Readonly(SOLE_USE_COUNT),
            RequestedUsage::Writable => CurrentUsage::Writable,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RequestedUsage {
    Readonly,
    Writable,
}

#[derive(Debug)]
struct Page {
    current_usage: CurrentUsage,
    contended_unique_weights: std::collections::BTreeSet<UniqueWeight>,
    //next_scheduled_task // reserved_task guaranteed_task
    //loaded account from Accounts db
    //comulative_cu for qos; i.e. track serialized cumulative keyed by addresses and bail out block
    //producing as soon as any one of cu from the executing thread reaches to the limit
}

impl Page {
    fn new(current_usage: CurrentUsage) -> Self {
        Self {
            current_usage,
            contended_unique_weights: Default::default(),
        }
    }
}

type AddressMap = std::collections::HashMap<Pubkey, MyRc<Page>>;
use by_address::ByAddress;
type AddressSet = std::collections::HashSet<ByAddress<MyRc<Page>>>;
type AddressMapEntry<'a, K, V> = std::collections::hash_map::Entry<'a, K, V>;

// needs ttl mechanism and prune
#[derive(Default)]
pub struct AddressBook {
    book: AddressMap,
    newly_uncontended_addresses: AddressSet,
}

impl AddressBook {
    #[inline(never)]
    fn attempt_lock_address(
        &mut self,
        from_runnable: bool,
        unique_weight: &UniqueWeight,
        attempt: &mut LockAttempt,
    ) {
        let LockAttempt {status, requested_usage, is_success} = attempt;

        let address = status.address2();
        match self.book.entry(*address) {
            AddressMapEntry::Vacant(book_entry) => {
                let page = MyRc::new(Page::new(CurrentUsage::renew(*requested_usage)));
                *status = LockAttemptStatus::AfterLookup(*address, MyRc::clone(&page));
                *is_success = true;
                book_entry.insert(page);
            }
            AddressMapEntry::Occupied(mut book_entry) => {
                let page = book_entry.get_mut();
                *status = LockAttemptStatus::AfterLookup(*address, MyRc::clone(&page));
                let mut page = unsafe { MyRcInner::get_mut_unchecked(page) };

                match page.current_usage {
                    CurrentUsage::Unused => {
                        page.current_usage = CurrentUsage::renew(*requested_usage);
                        *is_success = true;
                    }
                    CurrentUsage::Readonly(ref mut count) => match requested_usage {
                        RequestedUsage::Readonly => {
                            *count += 1;
                            *is_success = true;
                        }
                        RequestedUsage::Writable => {
                            if from_runnable {
                                Self::remember_address_contention(&mut page, unique_weight);
                            }
                            *is_success = false;
                        }
                    },
                    CurrentUsage::Writable => match requested_usage {
                        RequestedUsage::Readonly | RequestedUsage::Writable => {
                            if from_runnable {
                                Self::remember_address_contention(&mut page, unique_weight);
                            }
                            *is_success = false;
                        }
                    },
                }
            }
        }
    }

    #[inline(never)]
    fn remember_address_contention(page: &mut Page, unique_weight: &UniqueWeight) {
        page.contended_unique_weights.insert(*unique_weight);
    }

    #[inline(never)]
    fn forget_address_contention(&mut self, unique_weight: &UniqueWeight, a: &mut LockAttempt) {
        a.status.page().contended_unique_weights.remove(unique_weight);
    }

    fn ensure_unlock(&mut self, attempt: &mut LockAttempt) {
        if attempt.is_success() {
            self.unlock(attempt);
        }
    }

    #[inline(never)]
    fn unlock(&mut self, attempt: &mut LockAttempt) -> bool {
        debug_assert!(attempt.is_success());

        let mut newly_uncontended = false;
        let mut still_queued = false;

        let mut page = attempt.status.page();

        match &mut page.current_usage {
            CurrentUsage::Readonly(ref mut count) => match &attempt.requested_usage {
                RequestedUsage::Readonly => {
                    if *count == SOLE_USE_COUNT {
                        newly_uncontended = true;
                    } else {
                        *count -= 1;
                    }
                }
                RequestedUsage::Writable => unreachable!(),
            },
            CurrentUsage::Writable => match &attempt.requested_usage {
                RequestedUsage::Writable => {
                    newly_uncontended = true;
                }
                RequestedUsage::Readonly => unreachable!(),
            },
            CurrentUsage::Unused => unreachable!(),
        }

        if newly_uncontended {
            page.current_usage = CurrentUsage::Unused;
            if !page.contended_unique_weights.is_empty() {
                still_queued = true;
            }
        }

        still_queued
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Weight {
    // naming: Sequence Ordering?
    pub ix: u64, // index in ledger entry?
                   // gas fee
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
}

#[inline(never)]
fn attempt_lock_for_execution<'a>(
    from_runnable: bool,
    address_book: &mut AddressBook,
    unique_weight: &UniqueWeight,
    message_hash: &'a Hash,
    mut placeholder_attempts: Vec<LockAttempt>,
) -> (bool, Vec<LockAttempt>) {
    // no short-cuircuit; we at least all need to add to the contended queue
    let mut all_succeeded_so_far = true;

    for attempt in placeholder_attempts.iter_mut() {
        address_book.attempt_lock_address(from_runnable, unique_weight, attempt);
        if all_succeeded_so_far && attempt.is_failed() {
            all_succeeded_so_far = false;
        }
    }

    (all_succeeded_so_far, placeholder_attempts)
}

type PreprocessedTransaction = (SanitizedTransaction, Vec<LockAttempt>);
// multiplexed to reduce the futex syscal per tx down to minimum and to make the schduler to
// adaptive relative load between sigverify stage and execution substage
// switched from crossbeam_channel::select! due to observed poor performance
pub enum MultiplexedPayload {
    FromPrevious((Weight, Box<PreprocessedTransaction>)),
    FromPreviousBatched(Vec<Vec<Box<PreprocessedTransaction>>>),
    FromExecute(Box<ExecutionEnvironment>),
}

fn get_transaction_priority_details(tx: &SanitizedTransaction) -> u64 {
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
        (weight, tx): (Weight, Box<(SanitizedTransaction, Vec<LockAttempt>)>),
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
            Task { tx },
        );
        *unique_key -= 1;
    }

    #[inline(never)]
    fn get_newly_u_u_w<'a>(
        address: &'a Pubkey,
        address_book: &'a AddressBook,
    ) -> &'a std::collections::BTreeSet<UniqueWeight> {
        &address_book
            .book
            .get(address)
            .unwrap()
            .contended_unique_weights
    }

    #[inline(never)]
    fn get_weight_from_contended(address_book: &AddressBook) -> Option<UniqueWeight> {
        let mut heaviest_weight: Option<UniqueWeight> = None;
        //trace!("n u a len(): {}", address_book.newly_uncontended_addresses.len());
        for page in address_book.newly_uncontended_addresses.iter() {
            let newly_uncontended_unique_weights = &page.contended_unique_weights;
            if let Some(&weight) = newly_uncontended_unique_weights.last() {
                if let Some(current_heaviest_weight) = heaviest_weight {
                    if weight > current_heaviest_weight {
                        heaviest_weight = Some(weight);
                    }
                } else {
                    heaviest_weight = Some(weight);
                }
            }
        }
        heaviest_weight
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
            Self::get_weight_from_contended(address_book),
        ) {
            (Some(heaviest_runnable_entry), None) => {
                Some((Some(contended_queue), heaviest_runnable_entry))
            }
            (None, Some(weight_from_contended)) => Some((
                None,
                contended_queue.entry_to_execute(weight_from_contended),
            )),
            (Some(heaviest_runnable_entry), Some(weight_from_contended)) => {
                let weight_from_runnable = heaviest_runnable_entry.key();

                if weight_from_runnable > &weight_from_contended {
                    Some((Some(contended_queue), heaviest_runnable_entry))
                } else if &weight_from_contended > weight_from_runnable {
                    Some((
                        None,
                        contended_queue.entry_to_execute(weight_from_contended),
                    ))
                } else {
                    unreachable!(
                        "identical unique weights shouldn't exist in both runnable and contended"
                    )
                }
            }
            (None, None) => None,
        }
    }

    #[inline(never)]
    fn pop_from_queue_then_lock(
        runnable_queue: &mut TaskQueue,
        contended_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
    ) -> Option<(UniqueWeight, Task, Vec<LockAttempt>)> {
        for (reborrowed_contended_queue, mut queue_entry) in
            Self::select_next_task(runnable_queue, contended_queue, address_book)
        {
            let from_runnable = reborrowed_contended_queue.is_some();
            let unique_weight = *queue_entry.key();
            let next_task = queue_entry.get_mut();
            let message_hash = next_task.tx.0.message_hash();
            let placeholder_lock_attempts = std::mem::take(&mut next_task.tx.1);

            // plumb message_hash into StatusCache or implmenent our own for duplicate tx
            // detection?

            let (is_success, mut populated_lock_attempts) = attempt_lock_for_execution(
                from_runnable,
                address_book,
                &unique_weight,
                &message_hash,
                placeholder_lock_attempts,
            );

            if !is_success {
                //trace!("ensure_unlock_for_failed_execution(): {:?} {}", (&unique_weight, from_runnable), next_task.tx.signature());
                Self::ensure_unlock_for_failed_execution(
                    address_book,
                    &mut populated_lock_attempts,
                    from_runnable,
                );
                std::mem::swap(&mut next_task.tx.1, &mut populated_lock_attempts);
                if from_runnable {
                    reborrowed_contended_queue
                        .unwrap()
                        .add_to_schedule(*queue_entry.key(), queue_entry.remove());
                    // maybe run lightweight prune logic on contended_queue here.
                }
                continue;
            }

            Self::finalize_successful_lock_before_execution(
                address_book,
                &unique_weight,
                &mut populated_lock_attempts,
            );
            let task = queue_entry.remove();
            return Some((unique_weight, task, populated_lock_attempts));
        }

        None
    }

    #[inline(never)]
    // naming: relock_before_execution() / update_address_book() / update_uncontended_addresses()?
    fn finalize_successful_lock_before_execution(
        address_book: &mut AddressBook,
        unique_weight: &UniqueWeight,
        lock_attempts: &mut Vec<LockAttempt>,
    ) {
        for mut l in lock_attempts {
            // ensure to remove remaining refs of this unique_weight
            address_book.forget_address_contention(&unique_weight, &mut l);

            // revert because now contended again
            address_book.newly_uncontended_addresses.remove(&ByAddress(*l.status.page_rc()));
        }
    }

    #[inline(never)]
    fn ensure_unlock_for_failed_execution(
        address_book: &mut AddressBook,
        lock_attempts: &mut Vec<LockAttempt>,
        from_runnable: bool,
    ) {
        for l in lock_attempts {
            address_book.ensure_unlock(l);

            // revert because now contended again
            if !from_runnable {
                address_book.newly_uncontended_addresses.remove(&ByAddress(*l.status.page_rc()));
            }

            // todo: mem::forget and panic in LockAttempt::drop()
        }
    }

    #[inline(never)]
    fn unlock_after_execution(address_book: &mut AddressBook, lock_attempts: Vec<LockAttempt>) {
        for mut l in lock_attempts {
            let newly_uncontended_while_queued = address_book.unlock(&mut l);
            if newly_uncontended_while_queued {
                address_book.newly_uncontended_addresses.insert(ByAddress(MyRc::clone(l.status.page_rc())));
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
        weighted_tx: (Weight, Box<(SanitizedTransaction, Vec<LockAttempt>)>),
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
        from: &crossbeam_channel::Receiver<MultiplexedPayload>,
        to_execute_substage: &crossbeam_channel::Sender<Box<ExecutionEnvironment>>,
        // to_next_stage: &crossbeam_channel::Sender<Box<ExecutionEnvironment>>, // assume nonblocking
    ) {
        let mut executing_queue_count = 0;
        let mut current_unique_key = u64::max_value();

        loop {
            trace!("schedule_once!");

            let i = from.recv().unwrap();
            match i {
                MultiplexedPayload::FromPrevious(weighted_tx) => {
                    trace!("recv from previous");

                    Self::register_runnable_task(weighted_tx, runnable_queue, &mut current_unique_key);
                }
                MultiplexedPayload::FromPreviousBatched(vvv) => {
                    trace!("recv from previous");

                    for vv in vvv {
                        for v in vv {
                            let p = get_transaction_priority_details(&v.0);
                            Self::register_runnable_task((Weight { ix: p }, v), runnable_queue, &mut current_unique_key);
                            if executing_queue_count < max_executing_queue_count {
                                let maybe_ee =
                                    Self::schedule_next_execution(runnable_queue, contended_queue, address_book);
                                if let Some(ee) = maybe_ee {
                                    trace!("send to execute");
                                    executing_queue_count += 1;

                                    to_execute_substage.send(ee).unwrap();
                                }
                            }
                        }
                    }
                }
                MultiplexedPayload::FromExecute(mut processed_execution_environment) => {
                    trace!("recv from execute");
                    executing_queue_count -= 1;

                    Self::commit_result(&mut processed_execution_environment, address_book);
                    // async-ly propagate the result to rpc subsystems
                    // to_next_stage.send(processed_execution_environment).unwrap();
                }
            }

            while executing_queue_count < max_executing_queue_count {
                let maybe_ee =
                    Self::schedule_next_execution(runnable_queue, contended_queue, address_book);
                if let Some(ee) = maybe_ee {
                    trace!("send to execute");
                    executing_queue_count += 1;

                    to_execute_substage.send(ee).unwrap();
                } else {
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
