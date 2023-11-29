use {
    solana_sdk::transaction::SanitizedTransaction,
    std::{cell::UnsafeCell, collections::BTreeMap, sync::Arc},
};
use crate::cell::{SchedulerCell, Token};

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
struct TaskStatus(UnsafeCell<TaskStatusInner>);

mod cell {
    use std::cell::UnsafeCell;
    use std::marker::PhantomData;

    pub(super) struct SchedulerCell<V>(UnsafeCell<V>);
    
    impl<V> SchedulerCell<V> {
        // non-const to forbid unprotected sharing via static variables among threads.
        pub(super) fn new(value: V) -> Self {
            Self(UnsafeCell::new(value))
        }

        //pub(super) fn get<'token>(&self, _token: &'token mut Token) -> &'token mut V {
        pub(super) fn get<'t>(&self, _token: &'t mut Token) -> &'t mut V {
            unsafe { &mut *self.0.get() }
        }
    }

    unsafe impl<V> Send for SchedulerCell<V> {}
    unsafe impl<V> Sync for SchedulerCell<V> {}

    pub(super) struct Token(PhantomData<*mut ()>);

    impl Token {
        pub(super) unsafe fn assume_on_the_scheduler_thread() -> Self {
            Self(PhantomData)
        }
    }
}

pub fn aaa() {
    let mut token = unsafe { Token::assume_on_the_scheduler_thread() };
    let aa = &mut token;
    let bb = aa;
    let cell = SchedulerCell::new(23);
    let a = cell.get(aa);
    let b = cell.get(aa);
    dbg!((a,));
    //let () = std::thread::spawn(move || { a; }).join().unwrap();
}

impl TaskStatus {
    fn new(lock_attempts: Vec<LockAttempt>) -> Self {
        Self(UnsafeCell::new(TaskStatusInner {
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

    fn index_with_pages(self: &Arc<Self>) {
        for lock_attempt in self.lock_attempts_mut() {
            let requested_usage = lock_attempt.requested_usage;
            lock_attempt
                .page_mut()
                .insert_blocked_task(self.clone(), requested_usage);
        }
    }

    fn lock_attempts_mut(&self) -> &mut Vec<LockAttempt> {
        unsafe { &mut (*self.task_status.0.get()).lock_attempts }
    }

    fn uncontended(&self) -> &mut usize {
        unsafe { &mut (*self.task_status.0.get()).uncontended }
    }

    fn currently_contended(&self) -> bool {
        *self.uncontended() == 1
    }

    fn has_contended(&self) -> bool {
        *self.uncontended() > 0
    }

    fn mark_as_contended(&self) {
        *self.uncontended() = 1;
    }

    fn mark_as_uncontended(&self) {
        assert!(self.currently_contended());
        *self.uncontended() = 2;
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
    fn as_mut(&mut self) -> &mut PageInner {
        unsafe { &mut *self.0.get() }
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

    fn page_mut(&mut self) -> &mut PageInner {
        self.page.as_mut()
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

    fn heaviest_still_blocked_task(&self) -> Option<&(Task, RequestedUsage)> {
        self.blocked_tasks
            .values()
            .rev()
            .find(|(task, _)| task.currently_contended())
    }
}

static_assertions::const_assert_eq!(std::mem::size_of::<UnsafeCell<PageInner>>(), 32);

#[derive(Debug, Clone, Default)]
pub struct Page(Arc<UnsafeCell<PageInner>>);
static_assertions::const_assert_eq!(std::mem::size_of::<Page>(), 8);
unsafe impl Send for Page {}
unsafe impl Sync for Page {}

unsafe impl Sync for TaskStatus {}
type TaskQueue = BTreeMap<UniqueWeight, Task>;

#[derive(Default)]
pub struct SchedulingStateMachine {
    retryable_task_queue: TaskQueue,
    active_task_count: usize,
    handled_task_count: usize,
    reschedule_count: usize,
    rescheduled_task_count: usize,
    total_task_count: usize,
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

    pub fn schedule_new_task(&mut self, task: Task) -> Option<Task> {
        self.total_task_count += 1;
        self.active_task_count += 1;
        Self::try_lock_for_task((TaskSource::Runnable, task), &mut self.retryable_task_queue)
    }

    pub fn has_retryable_task(&self) -> bool {
        !self.retryable_task_queue.is_empty()
    }

    pub fn schedule_retryable_task(&mut self) -> Option<Task> {
        self.retryable_task_queue
            .pop_last()
            .and_then(|(_, task)| {
                self.reschedule_count += 1;
                Self::try_lock_for_task(
                    (TaskSource::Retryable, task),
                    &mut self.retryable_task_queue,
                )
            })
            .inspect(|_| {
                self.rescheduled_task_count += 1;
            })
    }

    pub fn deschedule_task(&mut self, task: &Task) {
        self.active_task_count -= 1;
        self.handled_task_count += 1;
        Self::unlock_after_execution(&task, &mut self.retryable_task_queue);
    }

    fn attempt_lock_for_execution(
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
            match Self::attempt_lock_address(unique_weight, attempt) {
                LockStatus::Succeded(usage) => {
                    if rollback_on_failure {
                        attempt.page_mut().current_usage = usage;
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
        this_unique_weight: &UniqueWeight,
        attempt: &mut LockAttempt,
    ) -> LockStatus {
        let requested_usage = attempt.requested_usage;
        let page = attempt.page_mut();

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

    fn unlock(attempt: &mut LockAttempt) -> bool {
        let mut is_unused_now = false;

        let requested_usage = attempt.requested_usage;
        let page = attempt.page_mut();

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
        (task_source, task): (TaskSource, Task),
        retryable_task_queue: &mut TaskQueue,
    ) -> Option<Task> {
        let (lock_count, usages) = Self::attempt_lock_for_execution(
            &task.unique_weight,
            &mut task.lock_attempts_mut(),
            &task_source,
        );

        if lock_count < task.lock_attempts_mut().len() {
            if matches!(task_source, TaskSource::Runnable) {
                Self::rollback_locking(&mut task.lock_attempts_mut()[..lock_count]);
                task.mark_as_contended();
                task.index_with_pages();
            }

            return None;
        }

        if matches!(task_source, TaskSource::Retryable) {
            for (usage, attempt) in usages.into_iter().zip(task.lock_attempts_mut()) {
                attempt.page_mut().current_usage = usage;
            }
            // as soon as next tack is succeeded in locking, trigger re-checks on read only
            // addresses so that more readonly transactions can be executed
            task.mark_as_uncontended();

            for read_only_lock_attempt in task
                .lock_attempts_mut()
                .iter_mut()
                .filter(|l| matches!(l.requested_usage, RequestedUsage::Readonly))
            {
                if let Some(heaviest_blocked_task) = read_only_lock_attempt
                    .page_mut()
                    .heaviest_still_blocked_task()
                    .and_then(|(task, requested_usage)| {
                        matches!(requested_usage, RequestedUsage::Readonly).then_some(task)
                    })
                {
                    retryable_task_queue
                        .entry(heaviest_blocked_task.unique_weight)
                        .or_insert_with(|| heaviest_blocked_task.clone());
                }
            }
        }

        Some(task)
    }

    fn rollback_locking(lock_attempts: &mut [LockAttempt]) {
        for lock_attempt in lock_attempts {
            Self::unlock(lock_attempt);
        }
    }

    fn unlock_after_execution(task: &Task, retryable_task_queue: &mut TaskQueue) {
        let unique_weight = &task.unique_weight;
        let should_remove = task.has_contended();

        for unlock_attempt in task.lock_attempts_mut() {
            if should_remove {
                unlock_attempt.page_mut().remove_blocked_task(unique_weight);
            }

            let is_unused_now = Self::unlock(unlock_attempt);
            if !is_unused_now {
                continue;
            }

            let heaviest_uncontended_now = unlock_attempt.page_mut().heaviest_still_blocked_task();
            if let Some((uncontended_task, _ru)) = heaviest_uncontended_now {
                retryable_task_queue
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
