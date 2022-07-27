#![feature(map_first_last)]

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

#[derive(Debug)]
pub struct ExecutionEnvironment {
    lock_attempts: Vec<LockAttempt>,
    //accounts: Vec<i8>,
    cu: usize,
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

#[derive(Debug)]
struct LockAttempt {
    address: Pubkey,
    is_success: bool,
    requested_usage: RequestedUsage,
}

impl LockAttempt {
    fn is_success(&self) -> bool {
        self.is_success
    }

    fn success(address: Pubkey, requested_usage: RequestedUsage) -> Self {
        Self {
            address,
            is_success: true,
            requested_usage,
        }
    }

    fn failure(address: Pubkey, requested_usage: RequestedUsage) -> Self {
        Self {
            address,
            is_success: false,
            requested_usage,
        }
    }
}

type UsageCount = usize;
const SOLE_USE_COUNT: UsageCount = 1;

#[derive(PartialEq)]
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
enum RequestedUsage {
    Readonly,
    Writable,
}

struct Page {
    current_usage: CurrentUsage,
    contended_unique_weights: std::collections::BTreeSet<UniqueWeight>,
    //next_scheduled_task // reserved_task guaranteed_task
    //loaded account
}

type AddressBookMap = std::collections::BTreeMap<Pubkey, Page>;

// needs ttl mechanism and prune
#[derive(Default)]
pub struct AddressBook {
    map: AddressBookMap,
    newly_uncontended_addresses: std::collections::BTreeSet<Pubkey>,
}

impl AddressBook {
    fn attempt_lock_address(
        &mut self,
        unique_weight: &UniqueWeight,
        address: Pubkey,
        requested_usage: RequestedUsage,
    ) -> LockAttempt {
        use std::collections::btree_map::Entry;

        match self.map.entry(address) {
            // unconditional success if it's initial access
            Entry::Vacant(entry) => {
                entry.insert(Page {
                    current_usage: CurrentUsage::renew(requested_usage),
                    contended_unique_weights: Default::default(),
                });
                LockAttempt::success(address, requested_usage)
            }
            Entry::Occupied(mut entry) => {
                let mut page = entry.get_mut();

                match &mut page.current_usage {
                    CurrentUsage::Unused => {
                        page.current_usage = CurrentUsage::renew(requested_usage);
                        LockAttempt::success(address, requested_usage)
                    }
                    CurrentUsage::Readonly(ref mut current_count) => {
                        match &requested_usage {
                            RequestedUsage::Readonly => {
                                *current_count += 1;
                                LockAttempt::success(address, requested_usage)
                            }
                            RequestedUsage::Writable => {
                                // skip insert if existing
                                page.contended_unique_weights
                                    .insert((*unique_weight).clone());
                                LockAttempt::failure(address, requested_usage)
                            }
                        }
                    }
                    CurrentUsage::Writable => {
                        match &requested_usage {
                            RequestedUsage::Readonly | RequestedUsage::Writable => {
                                // skip insert if existing
                                page.contended_unique_weights
                                    .insert((*unique_weight).clone());
                                LockAttempt::failure(address, requested_usage)
                            }
                        }
                    }
                }
            }
        }
    }

    fn remove_contended_unique_weight(
        &mut self,
        unique_weight: &UniqueWeight,
        address: &Pubkey,
    ) {
        use std::collections::btree_map::Entry;

        match self.map.entry(*address) {
            Entry::Vacant(entry) => unreachable!(),
            Entry::Occupied(mut entry) => {
                let mut page = entry.get_mut();
                page.contended_unique_weights.remove(unique_weight);
            }
        }
    }

    fn ensure_unlock(&mut self, attempt: &LockAttempt) {
        if attempt.is_success() {
            self.unlock(attempt);
        }
    }

    fn unlock(&mut self, attempt: &LockAttempt) -> bool {
        assert!(attempt.is_success());

        use std::collections::btree_map::Entry;
        let mut newly_uncontended = false;

        match self.map.entry(attempt.address) {
            Entry::Occupied(mut entry) => {
                let mut page = entry.get_mut();

                match &mut page.current_usage {
                    CurrentUsage::Readonly(ref mut current_count) => {
                        match &attempt.requested_usage {
                            RequestedUsage::Readonly => {
                                if *current_count == SOLE_USE_COUNT {
                                    newly_uncontended = true;
                                } else {
                                    *current_count -= 1;
                                }
                            }
                            RequestedUsage::Writable => unreachable!(),
                        }
                    }
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
                }
            }
            Entry::Vacant(entry) => {
                unreachable!()
            }
        }

        newly_uncontended
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Weight {
    // naming: Sequence Ordering?
    pub ix: usize, // index in ledger entry?
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UniqueWeight {
    // naming: Sequence Ordering?
    weight: Weight,
    // we can't use Transaction::message_hash because it's manipulatable to be favorous to the tx
    // submitter
    unique_key: Hash, // tie breaker? random noise? also for unique identification of txes?
                      // fee?
}

struct Bundle {
    // what about bundle1{tx1a, tx2} and bundle2{tx1b, tx2}?
}

#[derive(Debug)]
pub struct Task {
    pub tx: SanitizedTransaction, // actually should be Bundle
}

// RunnableQueue, ContendedQueue?
#[derive(Default)]
pub struct TaskQueue {
    map: std::collections::BTreeMap<UniqueWeight, Task>,
}

struct ContendedQueue {
    map: std::collections::BTreeMap<UniqueWeight, Task>,
}

impl TaskQueue {
    fn add(&mut self, unique_weight: UniqueWeight, task: Task) {
        //info!("TaskQueue::add(): {:?}", unique_weight);
        assert!(self.map.insert(unique_weight, task).is_none(), "identical shouldn't exist");
    }

    fn next_task_unique_weight(&self) -> Option<UniqueWeight> {
        self.map.last_key_value().map(|e| e.0.clone())
    }

    fn pop_next_task(&mut self) -> Option<(UniqueWeight, Task)> {
        self.map.pop_last()
    }
}

fn attempt_lock_for_execution<'a>(
    address_book: &mut AddressBook,
    unique_weight: &UniqueWeight,
    message_hash: &'a Hash,
    locks: &'a TransactionAccountLocks,
) -> Vec<LockAttempt> {
    // no short-cuircuit; we at least all need to add to the contended queue
    let mut writable_attempts = locks
        .writable
        .iter()
        .cloned()
        .map(|&a| address_book.attempt_lock_address(unique_weight, a, RequestedUsage::Writable))
        .collect::<Vec<_>>();

    let mut readonly_attempts = locks
        .readonly
        .iter()
        .cloned()
        .map(|&a| address_book.attempt_lock_address(unique_weight, a, RequestedUsage::Readonly))
        .collect::<Vec<_>>();

    writable_attempts.append(&mut readonly_attempts);
    writable_attempts
}

pub struct ScheduleStage {}

impl ScheduleStage {
    fn push_to_queue((weight, tx): (Weight, SanitizedTransaction), runnable_queue: &mut TaskQueue) {
        // manage randomness properly for future scheduling determinism
        let mut rng = rand::thread_rng();

        //let ix = 23;
        //let tx = bank
        //    .verify_transaction(
        //        tx,
        //        solana_sdk::transaction::TransactionVerificationMode::FullVerification,
        //    )
        //    .unwrap();
        //tx.foo();

        runnable_queue.add(
            UniqueWeight {
                weight,
                unique_key: solana_sdk::hash::new_rand(&mut rng),
            },
            Task { tx },
        );
    }

    fn select_next_task(
        runnable_queue: &mut TaskQueue,
        contended_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
    ) -> Option<(bool, UniqueWeight, Task)> {
        let mut unique_weights_by_address = std::collections::BTreeMap::<UniqueWeight, _>::new();
        for address in address_book.newly_uncontended_addresses.iter() {
            let newly_uncontended_unique_weights = &address_book
                .map
                .get(address)
                .unwrap()
                .contended_unique_weights;
            if !newly_uncontended_unique_weights.is_empty() {
                unique_weights_by_address.insert(
                    newly_uncontended_unique_weights.last().cloned().unwrap(),
                    newly_uncontended_unique_weights,
                );
            }
        }
        let heaviest_by_address = unique_weights_by_address.last_key_value();

        match (
            heaviest_by_address.map(|a| a.0.clone()),
            runnable_queue.next_task_unique_weight(),
        ) {
            (Some(weight_from_contended), Some(weight_from_runnable)) => {
                if weight_from_contended < weight_from_runnable {
                    runnable_queue.pop_next_task().map(|(uw, t)| (true, uw, t))
                } else if weight_from_contended > weight_from_runnable {
                    let heaviest_by_address = heaviest_by_address.unwrap();
                    let uw = heaviest_by_address.1.last().unwrap();
                    let task = contended_queue.map.remove(uw).unwrap();
                    Some((false, uw.clone(), task))
                } else {
                    unreachable!(
                        "identical unique weights shouldn't exist in both runnable and contended"
                    )
                }
            }
            (Some(weight_from_contended), None) => {
                let heaviest_by_address = heaviest_by_address.unwrap();
                let uw = heaviest_by_address.1.last().unwrap();
                let task = contended_queue.map.remove(uw).unwrap();
                Some((false, uw.clone(), task))
            }
            (None, Some(weight_from_runnable)) => {
                runnable_queue.pop_next_task().map(|(uw, t)| (true, uw, t))
            }
            (None, None) => None,
        }
    }

    fn pop_from_queue_then_lock(
        runnable_queue: &mut TaskQueue,
        contended_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
    ) -> Option<(UniqueWeight, Task, Vec<LockAttempt>)> {
        for (from_runnable, unique_weight, next_task) in
            Self::select_next_task(runnable_queue, contended_queue, address_book)
        {
            let message_hash = next_task.tx.message_hash();
            let locks = next_task.tx.get_account_locks().unwrap();

            // plumb message_hash into StatusCache or implmenent our own for duplicate tx
            // detection?

            let lock_attempts =
                attempt_lock_for_execution(address_book, &unique_weight, &message_hash, &locks);
            let is_success = lock_attempts.iter().all(|g| g.is_success());

            if is_success {
                return Some((unique_weight, next_task, lock_attempts));
            } else {
                //info!("ensure_unlock_for_failed_execution(): {:?} {}", (&unique_weight, from_runnable), next_task.tx.signature());
                Self::ensure_unlock_for_failed_execution(
                    address_book,
                    lock_attempts,
                    from_runnable,
                );
                contended_queue.add(unique_weight, next_task);
                return None;
            }
        }

        None
    }

    fn apply_successful_lock_before_execution(
        address_book: &mut AddressBook,
        unique_weight: UniqueWeight,
        lock_attempts: &Vec<LockAttempt>,
    ) {
        for l in lock_attempts {
            // ensure to remove remaining refs of this unique_weight
            address_book.remove_contended_unique_weight(&unique_weight, &l.address);

            // revert because now contended again
            address_book.newly_uncontended_addresses.remove(&l.address);
        }
    }

    fn ensure_unlock_for_failed_execution(
        address_book: &mut AddressBook,
        lock_attempts: Vec<LockAttempt>,
        from_runnable: bool,
    ) {
        for l in lock_attempts {
            address_book.ensure_unlock(&l);

            // revert because now contended again
            if !from_runnable {
                address_book.newly_uncontended_addresses.remove(&l.address);
            }

            // todo: mem::forget and panic in LockAttempt::drop()
        }
    }

    fn unlock_after_execution(address_book: &mut AddressBook, lock_attempts: Vec<LockAttempt>) {
        for l in lock_attempts {
            let newly_uncontended = address_book.unlock(&l);
            if newly_uncontended {
                address_book.newly_uncontended_addresses.insert(l.address);
            }

            // todo: mem::forget and panic in LockAttempt::drop()
        }
    }

    fn prepare_scheduled_execution(
        address_book: &mut AddressBook,
        unique_weight: UniqueWeight,
        task: Task,
        lock_attempts: Vec<LockAttempt>,
    ) -> ExecutionEnvironment {
        let mut rng = rand::thread_rng();
        // relock_before_execution() / update_address_book() / update_uncontended_addresses()?
        Self::apply_successful_lock_before_execution(address_book, unique_weight, &lock_attempts);
        // load account now from AccountsDb

        ExecutionEnvironment {
            lock_attempts,
            task,
            cu: rng.gen_range(0, 1000),
        }
    }

    fn commit_result(ee: &mut ExecutionEnvironment, address_book: &mut AddressBook) {
        let lock_attempts = std::mem::take(&mut ee.lock_attempts);
        // do par()-ly?
        Self::unlock_after_execution(address_book, lock_attempts);

        // par()-ly clone updated Accounts into address book
    }

    fn schedule_next_execution(
        runnable_queue: &mut TaskQueue,
        contended_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
    ) -> Option<ExecutionEnvironment> {
        let maybe_ee = Self::pop_from_queue_then_lock(runnable_queue, contended_queue, address_book)
            .map(|(uw, t, ll)| Self::prepare_scheduled_execution(address_book, uw, t, ll));
        maybe_ee
    }

    fn register_runnable_task(
        weighted_tx: (Weight, SanitizedTransaction),
        runnable_queue: &mut TaskQueue,
    ) {
        Self::push_to_queue(weighted_tx, runnable_queue)
    }

    pub fn schedule_once(
        runnable_queue: &mut TaskQueue,
        contended_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
        from_previous_stage: &crossbeam_channel::Receiver<(Weight, SanitizedTransaction)>,
        to_execute_substage: &crossbeam_channel::Sender<Option<ExecutionEnvironment>>, // ideally want to stop wrapping with Option<...>...
        from_execute_substage: &crossbeam_channel::Receiver<ExecutionEnvironment>,
        to_next_stage: &crossbeam_channel::Sender<ExecutionEnvironment>, // assume unbounded
    ) {
        use crossbeam_channel::select;

        let mut maybe_ee = None;
        let (s, r) = bounded(0);
        let mut was_some = false;

        loop {
            info!("schedule_once!");

        //if let Some(ee) = maybe_ee {
            select! {
                recv(from_previous_stage) -> weighted_tx => {
                    info!("recv from previous");
                    let weighted_tx = weighted_tx.unwrap();
                    Self::register_runnable_task(weighted_tx, runnable_queue);
                    if maybe_ee.is_none() {
                        maybe_ee = Self::schedule_next_execution(runnable_queue, contended_queue, address_book);
                    }
                }
                recv(from_execute_substage) -> processed_execution_environment => {
                    info!("recv from execute");
                    let mut processed_execution_environment = processed_execution_environment.unwrap();

                    Self::commit_result(&mut processed_execution_environment, address_book);

                    if maybe_ee.is_none() {
                        maybe_ee = Self::schedule_next_execution(runnable_queue, contended_queue, address_book);
                    }

                    // async-ly propagate the result to rpc subsystems
                    // to_next_stage is assumed to be non-blocking so, doesn't need to be one of select! handlers
                    to_next_stage.send(processed_execution_environment).unwrap();
                }
                send(maybe_ee.as_ref().map(|_| {
                    was_some = true;
                    to_execute_substage
                }).unwrap_or_else(|| {
                    was_some = false;
                    &s
                }), {
                    maybe_ee
                }) -> res => {
                    info!("send to execute: {}", was_some);
                    res.unwrap();
                    maybe_ee = None;
                }
                default => { 
                    info!("default");
                    std::thread::sleep(std::time::Duration::from_micros(50));
                    maybe_ee = Self::schedule_next_execution(runnable_queue, contended_queue, address_book);
                }
            }
        }
        /*} else {
            select! {
                recv(from_previous_stage) -> weighted_tx => {
                    let weighted_tx = weighted_tx.unwrap();
                    Self::register_runnable_task(weighted_tx, runnable_queue)
                }
                recv(from_execute_substage) -> processed_execution_environment => {
                    let mut processed_execution_environment = processed_execution_environment.unwrap();

                    Self::commit_result(&mut processed_execution_environment, address_book);

                    // async-ly propagate the result to rpc subsystems
                    // to_next_stage is assumed to be non-blocking so, doesn't need to be one of select! handlers
                    to_next_stage.send(processed_execution_environment).unwrap()
                }
                default => { std::thread::sleep(std::time::Duration::from_millis(1)) }
            }
        }*/
    }

    pub fn run(
        runnable_queue: &mut TaskQueue,
        contended_queue: &mut TaskQueue,
        address_book: &mut AddressBook,
        from_previous_stage: crossbeam_channel::Receiver<(Weight, SanitizedTransaction)>,
        to_execute_substage: crossbeam_channel::Sender<Option<ExecutionEnvironment>>, // ideally want to stop wrapping with Option<...>...
        from_execute_substage: crossbeam_channel::Receiver<ExecutionEnvironment>,
        to_next_stage: crossbeam_channel::Sender<ExecutionEnvironment>, // assume unbounded
    ) {
        let exit = true;
        while exit {
            Self::schedule_once(runnable_queue, contended_queue, address_book, &from_previous_stage, &to_execute_substage, &from_execute_substage, &to_next_stage);
        }
    }
}

struct ExecuteStage {
    //bank: Bank,
}

impl ExecuteStage {}
