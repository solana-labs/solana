#![allow(clippy::arithmetic_side_effects)]
use {
    crate::cell::{SchedulerCell, Token},
    solana_sdk::{pubkey::Pubkey, transaction::SanitizedTransaction},
    std::{collections::BTreeMap, sync::Arc},
};

type UsageCount = u32;
const SOLE_USE_COUNT: UsageCount = 1;

#[derive(Clone, Debug)]
enum LockStatus {
    Succeded(Usage),
    Failed,
}
static_assertions::const_assert_eq!(std::mem::size_of::<LockStatus>(), 8);

pub type Task = Arc<TaskInner>;
static_assertions::const_assert_eq!(std::mem::size_of::<Task>(), 8);

#[derive(Debug)]
struct TaskStatus {
    lock_attempts: Vec<LockAttempt>,
    uncontended: usize,
}

mod cell {
    use std::{cell::UnsafeCell, marker::PhantomData};

    #[derive(Debug, Default)]
    pub(super) struct SchedulerCell<V>(UnsafeCell<V>);

    impl<V> SchedulerCell<V> {
        // non-const to forbid unprotected sharing via static variables among threads.
        pub(super) fn new(value: V) -> Self {
            Self(UnsafeCell::new(value))
        }

        pub(super) fn borrow_mut<'t>(&self, _token: &'t mut Token<V>) -> &'t mut V {
            unsafe { &mut *self.0.get() }
        }

        pub(super) fn borrow<'t>(&self, _token: &'t Token<V>) -> &'t V {
            unsafe { &*self.0.get() }
        }
    }

    unsafe impl<V> Send for SchedulerCell<V> {}
    unsafe impl<V> Sync for SchedulerCell<V> {}

    pub(super) struct Token<T>(PhantomData<*mut T>);

    impl<T> Token<T> {
        pub(super) unsafe fn assume_on_the_scheduler_thread() -> Self {
            Self(PhantomData)
        }
    }
}

type PageToken = Token<PageInner>;
static_assertions::const_assert_eq!(std::mem::size_of::<PageToken>(), 0);

type TaskToken = Token<TaskStatus>;
static_assertions::const_assert_eq!(std::mem::size_of::<TaskToken>(), 0);

impl TaskStatus {
    fn new(lock_attempts: Vec<LockAttempt>) -> Self {
        Self {
            lock_attempts,
            uncontended: 0,
        }
    }
}

#[derive(Debug)]
pub struct TaskInner {
    // put this field out of this struct for maximum space efficiency?
    unique_weight: UniqueWeight,
    transaction: SanitizedTransaction, // actually should be Bundle
    task_status: SchedulerCell<TaskStatus>,
}

impl SchedulingStateMachine {
    pub fn create_task(
        transaction: SanitizedTransaction,
        index: usize,
        mut page_loader: &mut impl FnMut(Pubkey) -> Page,
    ) -> Task {
        let locks = transaction.get_account_locks_unchecked();

        let writable_locks = locks
            .writable
            .iter()
            .map(|address| (address, RequestedUsage::Writable));
        let readonly_locks = locks
            .readonly
            .iter()
            .map(|address| (address, RequestedUsage::Readonly));

        let locks = writable_locks
            .chain(readonly_locks)
            .map(|(address, requested_usage)| {
                LockAttempt::new(page_loader(**address), requested_usage)
            })
            .collect();

        let unique_weight = UniqueWeight::max_value() - index as UniqueWeight;

        Task::new(TaskInner {
            unique_weight,
            transaction,
            task_status: SchedulerCell::new(TaskStatus::new(locks)),
        })
    }
}

impl TaskInner {
    pub fn transaction(&self) -> &SanitizedTransaction {
        &self.transaction
    }

    fn lock_attempts_mut<'t>(&self, task_token: &'t mut TaskToken) -> &'t mut Vec<LockAttempt> {
        &mut self.task_status.borrow_mut(task_token).lock_attempts
    }

    fn lock_attempts<'t>(&self, task_token: &'t TaskToken) -> &'t Vec<LockAttempt> {
        &self.task_status.borrow(task_token).lock_attempts
    }

    fn uncontended_mut<'t>(&self, task_token: &'t mut TaskToken) -> &'t mut usize {
        &mut self.task_status.borrow_mut(task_token).uncontended
    }

    fn uncontended_ref<'t>(&self, task_token: &'t TaskToken) -> &'t usize {
        &self.task_status.borrow(task_token).uncontended
    }

    fn currently_contended(&self, task_token: &TaskToken) -> bool {
        *self.uncontended_ref(task_token) == 1
    }

    fn has_contended(&self, task_token: &TaskToken) -> bool {
        *self.uncontended_ref(task_token) > 0
    }

    fn mark_as_contended(&self, task_token: &mut TaskToken) {
        *self.uncontended_mut(task_token) = 1;
    }

    fn mark_as_uncontended(&self, task_token: &mut TaskToken) {
        assert!(self.currently_contended(task_token));
        *self.uncontended_mut(task_token) = 2;
    }

    pub fn task_index(&self) -> usize {
        (UniqueWeight::max_value() - self.unique_weight) as usize
    }
}

#[derive(Debug)]
struct LockAttempt {
    page: Page,
    requested_usage: RequestedUsage,
    uncommited_usage: Usage,
}

impl LockAttempt {
    fn new(page: Page, requested_usage: RequestedUsage) -> Self {
        Self {
            page,
            requested_usage,
            uncommited_usage: Usage::default(),
        }
    }

    fn page_mut<'t>(&self, page_token: &'t mut PageToken) -> &'t mut PageInner {
        self.page.0.borrow_mut(page_token)
    }
}

#[derive(Copy, Clone, Debug, Default)]
enum Usage {
    #[default]
    Unused,
    Readonly(UsageCount),
    Writable,
}
static_assertions::const_assert_eq!(std::mem::size_of::<Usage>(), 8);

impl Usage {
    fn renew(requested_usage: RequestedUsage) -> Self {
        match requested_usage {
            RequestedUsage::Readonly => Usage::Readonly(SOLE_USE_COUNT),
            RequestedUsage::Writable => Usage::Writable,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RequestedUsage {
    Readonly,
    Writable,
}

#[derive(Debug, Default)]
struct PageInner {
    current_usage: Usage,
    blocked_tasks: BTreeMap<UniqueWeight, (Task, RequestedUsage)>,
}

impl PageInner {
    fn insert_blocked_task(&mut self, task: Task, requested_usage: RequestedUsage) {
        let pre_existed = self
            .blocked_tasks
            .insert(task.unique_weight, (task, requested_usage));
        assert!(pre_existed.is_none());
    }

    fn remove_blocked_task(&mut self, unique_weight: UniqueWeight) {
        let removed_entry = self.blocked_tasks.remove(&unique_weight);
        assert!(removed_entry.is_some());
    }

    fn heaviest_blocked_writing_task_weight(&self) -> Option<UniqueWeight> {
        self.blocked_tasks
            .values()
            .rev()
            .find_map(|(task, requested_usage)| {
                matches!(requested_usage, RequestedUsage::Writable).then_some(task.unique_weight)
            })
    }

    fn heaviest_blocked_task(&mut self) -> Option<UniqueWeight> {
        self.blocked_tasks.last_entry().map(|entry| *entry.key())
    }

    fn heaviest_still_blocked_task(
        &self,
        task_token: &TaskToken,
    ) -> Option<&(Task, RequestedUsage)> {
        self.blocked_tasks
            .values()
            .rev()
            .find(|(task, _)| task.currently_contended(task_token))
    }
}

static_assertions::const_assert_eq!(std::mem::size_of::<SchedulerCell<PageInner>>(), 32);

#[derive(Debug, Clone, Default)]
pub struct Page(Arc<SchedulerCell<PageInner>>);
static_assertions::const_assert_eq!(std::mem::size_of::<Page>(), 8);

type TaskQueue = BTreeMap<UniqueWeight, Task>;

pub struct SchedulingStateMachine {
    retryable_task_queue: TaskQueue,
    active_task_count: usize,
    handled_task_count: usize,
    reschedule_count: usize,
    rescheduled_task_count: usize,
    total_task_count: usize,
    task_token: TaskToken,
    page_token: PageToken,
}

impl SchedulingStateMachine {
    pub fn is_empty(&self) -> bool {
        self.active_task_count == 0
    }

    pub fn retryable_task_count(&self) -> usize {
        self.retryable_task_queue.len()
    }

    pub fn active_task_count(&self) -> usize {
        self.active_task_count
    }

    pub fn handled_task_count(&self) -> usize {
        self.handled_task_count
    }

    pub fn reschedule_count(&self) -> usize {
        self.reschedule_count
    }

    pub fn rescheduled_task_count(&self) -> usize {
        self.rescheduled_task_count
    }

    pub fn total_task_count(&self) -> usize {
        self.total_task_count
    }

    pub fn schedule_task(&mut self, task: Task) -> Option<Task> {
        self.total_task_count += 1;
        self.active_task_count += 1;
        self.try_lock_for_task(TaskSource::Runnable, task)
    }

    pub fn has_retryable_task(&self) -> bool {
        !self.retryable_task_queue.is_empty()
    }

    pub fn schedule_retryable_task(&mut self) -> Option<Task> {
        self.retryable_task_queue
            .pop_last()
            .and_then(|(_, task)| {
                self.reschedule_count += 1;
                self.try_lock_for_task(TaskSource::Retryable, task)
            })
            .map(|task| {
                self.rescheduled_task_count += 1;
                task
            })
    }

    pub fn deschedule_task(&mut self, task: &Task) {
        self.active_task_count -= 1;
        self.handled_task_count += 1;
        self.unlock_after_execution(task);
    }

    fn attempt_lock_for_execution(
        page_token: &mut PageToken,
        unique_weight: UniqueWeight,
        lock_attempts: &mut [LockAttempt],
        task_source: &TaskSource,
    ) -> usize {
        let rollback_on_failure = matches!(task_source, TaskSource::Runnable);

        let mut lock_count = 0;

        for attempt in lock_attempts.iter_mut() {
            match Self::attempt_lock_address(page_token, unique_weight, attempt) {
                LockStatus::Succeded(usage) => {
                    if rollback_on_failure {
                        attempt.page_mut(page_token).current_usage = usage;
                    } else {
                        attempt.uncommited_usage = usage;
                    }
                    lock_count += 1;
                }
                LockStatus::Failed => break,
            }
        }

        lock_count
    }

    fn attempt_lock_address(
        page_token: &mut PageToken,
        this_unique_weight: UniqueWeight,
        attempt: &mut LockAttempt,
    ) -> LockStatus {
        let requested_usage = attempt.requested_usage;
        let page = attempt.page_mut(page_token);

        let mut lock_status = match page.current_usage {
            Usage::Unused => LockStatus::Succeded(Usage::renew(requested_usage)),
            Usage::Readonly(count) => match requested_usage {
                RequestedUsage::Readonly => LockStatus::Succeded(Usage::Readonly(count + 1)),
                RequestedUsage::Writable => LockStatus::Failed,
            },
            Usage::Writable => LockStatus::Failed,
        };

        if matches!(lock_status, LockStatus::Succeded(_)) {
            let no_heavier_other_tasks =
                // this unique_weight is the heaviest one among all of other tasks blocked on this
                // page.
                (page
                    .heaviest_blocked_task()
                    .map(|existing_unique_weight| this_unique_weight == existing_unique_weight)
                    .unwrap_or(true)) ||
                // this _read-only_ unique_weight is heavier than any of contened write locks.
                (matches!(requested_usage, RequestedUsage::Readonly) && page
                    .heaviest_blocked_writing_task_weight()
                    .map(|existing_unique_weight| this_unique_weight > existing_unique_weight)
                    .unwrap_or(true))
            ;

            if !no_heavier_other_tasks {
                lock_status = LockStatus::Failed
            }
        }
        lock_status
    }

    fn unlock(page_token: &mut PageToken, attempt: &LockAttempt) -> bool {
        let mut is_unused_now = false;

        let requested_usage = attempt.requested_usage;
        let page = attempt.page_mut(page_token);

        match &mut page.current_usage {
            Usage::Readonly(ref mut count) => match requested_usage {
                RequestedUsage::Readonly => {
                    if *count == SOLE_USE_COUNT {
                        is_unused_now = true;
                    } else {
                        *count -= 1;
                    }
                }
                RequestedUsage::Writable => unreachable!(),
            },
            Usage::Writable => match requested_usage {
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

    fn try_lock_for_task(&mut self, task_source: TaskSource, task: Task) -> Option<Task> {
        let lock_count = Self::attempt_lock_for_execution(
            &mut self.page_token,
            task.unique_weight,
            task.lock_attempts_mut(&mut self.task_token),
            &task_source,
        );

        if lock_count < task.lock_attempts_mut(&mut self.task_token).len() {
            if matches!(task_source, TaskSource::Runnable) {
                self.rollback_locking(&task, lock_count);
                task.mark_as_contended(&mut self.task_token);
                self.index_task_with_pages(&task);
            }

            return None;
        }

        if matches!(task_source, TaskSource::Retryable) {
            for attempt in task.lock_attempts_mut(&mut self.task_token) {
                attempt.page_mut(&mut self.page_token).current_usage = attempt.uncommited_usage;
            }
            // as soon as next tack is succeeded in locking, trigger re-checks on read only
            // addresses so that more readonly transactions can be executed
            task.mark_as_uncontended(&mut self.task_token);

            for read_only_lock_attempt in task
                .lock_attempts(&self.task_token)
                .iter()
                .filter(|l| matches!(l.requested_usage, RequestedUsage::Readonly))
            {
                if let Some(heaviest_blocked_task) = read_only_lock_attempt
                    .page_mut(&mut self.page_token)
                    .heaviest_still_blocked_task(&self.task_token)
                    .and_then(|(task, requested_usage)| {
                        matches!(requested_usage, RequestedUsage::Readonly).then_some(task)
                    })
                {
                    self.retryable_task_queue
                        .entry(heaviest_blocked_task.unique_weight)
                        .or_insert_with(|| heaviest_blocked_task.clone());
                }
            }
        }

        Some(task)
    }

    fn rollback_locking(&mut self, task: &Task, lock_count: usize) {
        for lock_attempt in &task.lock_attempts_mut(&mut self.task_token)[..lock_count] {
            Self::unlock(&mut self.page_token, lock_attempt);
        }
    }

    fn index_task_with_pages(&mut self, task: &Task) {
        for lock_attempt in task.lock_attempts_mut(&mut self.task_token) {
            let requested_usage = lock_attempt.requested_usage;
            lock_attempt
                .page_mut(&mut self.page_token)
                .insert_blocked_task(task.clone(), requested_usage);
        }
    }

    fn unlock_after_execution(&mut self, task: &Task) {
        let should_remove = task.has_contended(&self.task_token);

        for unlock_attempt in task.lock_attempts(&self.task_token) {
            if should_remove {
                unlock_attempt
                    .page_mut(&mut self.page_token)
                    .remove_blocked_task(task.unique_weight);
            }

            let is_unused_now = Self::unlock(&mut self.page_token, unlock_attempt);
            if !is_unused_now {
                continue;
            }

            let heaviest_uncontended_now = unlock_attempt
                .page_mut(&mut self.page_token)
                .heaviest_still_blocked_task(&self.task_token);
            if let Some((uncontended_task, _ru)) = heaviest_uncontended_now {
                self.retryable_task_queue
                    .entry(uncontended_task.unique_weight)
                    .or_insert_with(|| uncontended_task.clone());
            }
        }
    }
}

impl Default for SchedulingStateMachine {
    fn default() -> Self {
        Self {
            retryable_task_queue: TaskQueue::default(),
            active_task_count: 0,
            handled_task_count: 0,
            reschedule_count: 0,
            rescheduled_task_count: 0,
            total_task_count: 0,
            task_token: unsafe { TaskToken::assume_on_the_scheduler_thread() },
            page_token: unsafe { PageToken::assume_on_the_scheduler_thread() },
        }
    }
}

enum TaskSource {
    Runnable,
    Retryable,
}

type UniqueWeight = u64;

#[cfg(test)]
mod tests {
    use super::*;
    use {
        assert_matches::assert_matches,
        iai_callgrind::{
            client_requests::callgrind::toggle_collect, library_benchmark, library_benchmark_group,
            main,
        },
        solana_sdk::{
            instruction::{AccountMeta, Instruction},
            message::Message,
            pubkey::Pubkey,
            signature::Signer,
            signer::keypair::Keypair,
            system_transaction,
            transaction::{Result, SanitizedTransaction, Transaction},
        },
        std::hint::black_box,
    };
    use std::collections::HashMap;

    fn simplest_transaction() -> SanitizedTransaction {
        let payer = Keypair::new();
        let message = Message::new(&[], Some(&payer.pubkey()));
        let unsigned = Transaction::new_unsigned(message);
        SanitizedTransaction::from_transaction_for_tests(unsigned)
    }

    fn readonly_transaction() -> SanitizedTransaction {
        let payer = Keypair::new();
        let instruction = Instruction {
            program_id: Pubkey::default(),
            accounts: vec![AccountMeta::new_readonly(Keypair::new().pubkey(), false)],
            data: vec![],
        };
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let unsigned = Transaction::new_unsigned(message);
        SanitizedTransaction::from_transaction_for_tests(unsigned)
    }

    #[test]
    fn test_scheduling_state_machine_default() {
        let state_machine = SchedulingStateMachine::default();
        assert_eq!(state_machine.active_task_count(), 0);
        assert_eq!(state_machine.total_task_count(), 0);
    }

    #[test]
    fn test_create_task() {
        let sanitized = simplest_transaction();
        let task = SchedulingStateMachine::create_task(sanitized.clone(), 3, &mut |_| Page::default());
        assert_eq!(task.task_index(), 3);
        assert_eq!(task.transaction(), &sanitized);
    }

    fn create_address_loader() -> impl FnMut(Pubkey) -> Page {
        let mut pages =
            HashMap::<Pubkey, Page>::new();

        move |address| pages.entry(address).or_default().clone()
    }

    #[test]
    fn test_schedule_non_conflicting_task() {
        let sanitized = simplest_transaction();
        let task = SchedulingStateMachine::create_task(sanitized.clone(), 3, &mut |_| Page::default());

        let mut state_machine = SchedulingStateMachine::default();
        let task = state_machine.schedule_task(task).unwrap();
        assert_eq!(state_machine.active_task_count(), 1);
        assert_eq!(state_machine.total_task_count(), 1);
        state_machine.deschedule_task(&task);
        assert_eq!(state_machine.active_task_count(), 0);
        assert_eq!(state_machine.total_task_count(), 1);
        drop(task);
    }

    #[test]
    fn test_schedule_conflicting_tasks() {
        let sanitized = simplest_transaction();
        let mut address_loader = create_address_loader();
        let task = SchedulingStateMachine::create_task(sanitized.clone(), 3, &mut address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 4, &mut address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        state_machine.schedule_task(task).unwrap();
        assert_matches!(state_machine.schedule_task(task2), None);
    }

    #[test]
    fn test_schedule_non_conflicting_readonly_tasks() {
        let sanitized = readonly_transaction();
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 3, &mut |_| Page::default());

        let mut state_machine = SchedulingStateMachine::default();
        let task1 = state_machine.schedule_task(task1).unwrap();
        assert_eq!(state_machine.active_task_count(), 1);
        assert_eq!(state_machine.total_task_count(), 1);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.active_task_count(), 0);
        assert_eq!(state_machine.total_task_count(), 1);
        drop(task1);
    }
}
