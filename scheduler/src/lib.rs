use {
    solana_sdk::transaction::SanitizedTransaction,
    std::{collections::BTreeMap, sync::Arc},
};
use crate::cell::{SchedulerCell, Token, Token2};

type UsageCount = u32;
const SOLE_USE_COUNT: UsageCount = 1;

#[derive(Clone, Debug)]
enum LockStatus {
    Succeded(Usage),
    Failed,
}

pub type Task = Arc<TaskInner>;

#[derive(Debug)]
struct TaskStatusInner {
    lock_attempts: Vec<LockAttempt>,
    uncontended: usize,
}

#[derive(Debug)]
struct TaskStatus(SchedulerCell<TaskStatusInner>);

mod cell {
    use std::cell::UnsafeCell;
    use std::marker::PhantomData;

    #[derive(Debug, Default)]
    pub(super) struct SchedulerCell<V>(UnsafeCell<V>);
    
    impl<V> SchedulerCell<V> {
        // non-const to forbid unprotected sharing via static variables among threads.
        pub(super) fn new(value: V) -> Self {
            Self(UnsafeCell::new(value))
        }

        pub(super) fn get<'t>(&self, _token: &'t mut TokenNew<V>) -> &'t mut V {
            unsafe { &mut *self.0.get() }
        }

        pub(super) fn get22<'t>(&self, _token: &'t TokenNew<V>) -> &'t V {
            unsafe { &*self.0.get() }
        }
    }

    unsafe impl<V> Send for SchedulerCell<V> {}
    unsafe impl<V> Sync for SchedulerCell<V> {}

    pub(super) struct TokenNew<T>(PhantomData<(T, *mut ())>);

    impl<T> TokenNew<T> {
        pub(super) unsafe fn assume_on_the_scheduler_thread() -> Self {
            Self(PhantomData)
        }
    }

    pub(super) type Token2 = TokenNew<crate::PageInner>;

    pub(super) type Token = TokenNew<crate::TaskStatusInner>;
    static_assertions::const_assert_eq!(std::mem::size_of::<Token>(), 0);
}

impl TaskStatus {
    fn new(lock_attempts: Vec<LockAttempt>) -> Self {
        Self(SchedulerCell::new(TaskStatusInner {
            lock_attempts,
            uncontended: 0,
        }))
    }
}

#[derive(Debug)]
pub struct TaskInner {
    unique_weight: UniqueWeight,
    tx: SanitizedTransaction, // actually should be Bundle
    task_status: TaskStatus,
}

impl SchedulingStateMachine {
    pub fn create_task(
        index: usize,
        tx: SanitizedTransaction,
        lock_attempts: Vec<LockAttempt>,
    ) -> Task {
        let unique_weight = UniqueWeight::max_value() - index as UniqueWeight;
        Task::new(TaskInner {
            unique_weight,
            tx,
            task_status: TaskStatus::new(lock_attempts),
        })
    }
}

impl TaskInner {
    pub fn transaction(&self) -> &SanitizedTransaction {
        &self.tx
    }

    fn lock_attempts_mut<'t>(&self, token: &'t mut Token) -> &'t mut Vec<LockAttempt> {
        &mut self.task_status.0.get(token).lock_attempts
    }

    fn lock_attempts<'t>(&self, token: &'t Token) -> &'t Vec<LockAttempt> {
        &self.task_status.0.get22(token).lock_attempts
    }

    fn uncontended<'t>(&self, token: &'t mut Token) -> &'t mut usize {
        &mut self.task_status.0.get(token).uncontended
    }

    fn uncontended22<'t>(&self, token: &'t Token) -> &'t usize {
        &self.task_status.0.get22(token).uncontended
    }

    fn currently_contended(&self, token: &Token) -> bool {
        *self.uncontended22(token) == 1
    }

    fn has_contended(&self, token: &mut Token) -> bool {
        *self.uncontended(token) > 0
    }

    fn mark_as_contended(&self, token: &mut Token) {
        *self.uncontended(token) = 1;
    }

    fn mark_as_uncontended(&self, token: &mut Token) {
        assert!(self.currently_contended(token));
        *self.uncontended(token) = 2;
    }

    pub fn task_index(&self) -> usize {
        (UniqueWeight::max_value() - self.unique_weight) as usize
    }
}

#[derive(Debug)]
pub struct LockAttempt {
    page: Page,
    requested_usage: RequestedUsage,
}

impl Page {
    fn as_mut<'t>(&self, token: &'t mut Token2) -> &'t mut PageInner {
        self.0.get(token)
    }
}

impl LockAttempt {
    pub fn readonly(page: Page) -> Self {
        Self {
            page,
            requested_usage: RequestedUsage::Readonly,
        }
    }

    pub fn writable(page: Page) -> Self {
        Self {
            page,
            requested_usage: RequestedUsage::Writable,
        }
    }

    fn page_mut<'t>(&self, token: &'t mut Token2) -> &'t mut PageInner {
        self.page.as_mut(token)
    }
}

#[derive(Copy, Clone, Debug, Default)]
enum Usage {
    #[default]
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

    fn remove_blocked_task(&mut self, unique_weight: &UniqueWeight) {
        let removed_entry = self.blocked_tasks.remove(unique_weight);
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

    fn heaviest_still_blocked_task(&self, token: &Token) -> Option<&(Task, RequestedUsage)> {
        self.blocked_tasks
            .values()
            .rev()
            .find(|(task, _)| task.currently_contended(token))
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
    token: Token,
    token2: Token2,
}

impl SchedulingStateMachine {
    pub fn new() -> Self {
        Self {
            retryable_task_queue: TaskQueue::default(),
            active_task_count: 0,
            handled_task_count: 0,
            reschedule_count: 0,
            rescheduled_task_count: 0,
            total_task_count: 0,
            token: unsafe { Token::assume_on_the_scheduler_thread() },
            token2: unsafe { Token2::assume_on_the_scheduler_thread() },
        }
    }

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

    pub fn schedule_new_task(&mut self, task: Task) -> Option<Task> {
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
            .inspect(|_| {
                self.rescheduled_task_count += 1;
            })
    }

    pub fn deschedule_task(&mut self, task: &Task) {
        self.active_task_count -= 1;
        self.handled_task_count += 1;
        self.unlock_after_execution(&task);
    }

    fn attempt_lock_for_execution(
        token2: &mut Token2,
        unique_weight: &UniqueWeight,
        lock_attempts: &mut [LockAttempt],
        task_source: &TaskSource,
    ) -> (usize, Vec<Usage>) {
        let rollback_on_failure = matches!(task_source, TaskSource::Runnable);

        let mut lock_count = 0;
        let mut uncommited_usages = Vec::with_capacity(if rollback_on_failure {
            0
        } else {
            lock_attempts.len()
        });

        for attempt in lock_attempts.iter_mut() {
            match Self::attempt_lock_address(token2, unique_weight, attempt) {
                LockStatus::Succeded(usage) => {
                    if rollback_on_failure {
                        attempt.page_mut(token2).current_usage = usage;
                    } else {
                        uncommited_usages.push(usage);
                    }
                    lock_count += 1;
                }
                LockStatus::Failed => {
                    break;
                }
            }
        }

        (lock_count, uncommited_usages)
    }

    fn attempt_lock_address(
        token2: &mut Token2,
        this_unique_weight: &UniqueWeight,
        attempt: &mut LockAttempt,
    ) -> LockStatus {
        let requested_usage = attempt.requested_usage;
        let page = attempt.page_mut(token2);

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
                    .map(|existing_unique_weight| *this_unique_weight == existing_unique_weight)
                    .unwrap_or(true)) ||
                // this _read-only_ unique_weight is heavier than any of contened write locks.
                (matches!(requested_usage, RequestedUsage::Readonly) && page
                    .heaviest_blocked_writing_task_weight()
                    .map(|existing_unique_weight| *this_unique_weight > existing_unique_weight)
                    .unwrap_or(true))
            ;

            if !no_heavier_other_tasks {
                lock_status = LockStatus::Failed
            }
        }
        lock_status
    }

    fn unlock(token2: &mut Token2, attempt: &LockAttempt) -> bool {
        let mut is_unused_now = false;

        let requested_usage = attempt.requested_usage;
        let page = attempt.page_mut(token2);

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

    fn try_lock_for_task(
        &mut self,
        task_source: TaskSource,
        task: Task,
    ) -> Option<Task> {
        let (lock_count, usages) = Self::attempt_lock_for_execution(
            &mut self.token2,
            &task.unique_weight,
            &mut task.lock_attempts_mut(&mut self.token),
            &task_source,
        );

        if lock_count < task.lock_attempts_mut(&mut self.token).len() {
            if matches!(task_source, TaskSource::Runnable) {
                self.rollback_locking(&task, lock_count);
                task.mark_as_contended(&mut self.token);
                self.index_task_with_pages(&task);
            }

            return None;
        }

        if matches!(task_source, TaskSource::Retryable) {
            for (usage, attempt) in usages.into_iter().zip(task.lock_attempts_mut(&mut self.token)) {
                attempt.page_mut(&mut self.token2).current_usage = usage;
            }
            // as soon as next tack is succeeded in locking, trigger re-checks on read only
            // addresses so that more readonly transactions can be executed
            task.mark_as_uncontended(&mut self.token);

            for read_only_lock_attempt in task
                .lock_attempts(&self.token)
                .iter()
                .filter(|l| matches!(l.requested_usage, RequestedUsage::Readonly))
            {
                if let Some(heaviest_blocked_task) = read_only_lock_attempt
                    .page_mut(&mut self.token2)
                    .heaviest_still_blocked_task(&self.token)
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
        for lock_attempt in &task.lock_attempts_mut(&mut self.token)[..lock_count] {
            Self::unlock(&mut self.token2, lock_attempt);
        }
    }

    fn index_task_with_pages(&mut self, task: &Task) {
        for lock_attempt in task.lock_attempts_mut(&mut self.token) {
            let requested_usage = lock_attempt.requested_usage;
            lock_attempt
                .page_mut(&mut self.token2)
                .insert_blocked_task(task.clone(), requested_usage);
        }
    }

    fn unlock_after_execution(&mut self, task: &Task) {
        let unique_weight = &task.unique_weight;
        let should_remove = task.has_contended(&mut self.token);

        for unlock_attempt in task.lock_attempts(&self.token) {
            if should_remove {
                unlock_attempt.page_mut(&mut self.token2).remove_blocked_task(unique_weight);
            }

            let is_unused_now = Self::unlock(&mut self.token2, unlock_attempt);
            if !is_unused_now {
                continue;
            }

            let heaviest_uncontended_now = unlock_attempt.page_mut(&mut self.token2).heaviest_still_blocked_task(&self.token);
            if let Some((uncontended_task, _ru)) = heaviest_uncontended_now {
                self.retryable_task_queue
                    .entry(uncontended_task.unique_weight)
                    .or_insert_with(|| uncontended_task.clone());
            }
        }
    }
}

enum TaskSource {
    Runnable,
    Retryable,
}

type UniqueWeight = u64;
