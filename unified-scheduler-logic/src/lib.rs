#[cfg(feature = "dev-context-only-utils")]
use qualifier_attr::{field_qualifiers, qualifiers};
use {
    crate::utils::{ShortCounter, Token, TokenCell, TokenTrait},
    solana_sdk::{pubkey::Pubkey, transaction::SanitizedTransaction},
    static_assertions::const_assert_eq,
    std::{
        collections::{BTreeMap, VecDeque},
        mem,
        sync::Arc,
    },
};

#[derive(Clone, Debug, Default)]
enum LockStatus {
    Succeded(Usage),
    #[default]
    Failed,
}
const_assert_eq!(mem::size_of::<LockStatus>(), 8);

pub type Task = Arc<TaskInner>;
const_assert_eq!(mem::size_of::<Task>(), 8);

mod utils {
    #[cfg(feature = "dev-context-only-utils")]
    use qualifier_attr::qualifiers;
    use std::{cell::UnsafeCell, marker::PhantomData};

    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
    #[derive(Debug, Clone, Copy)]
    pub(super) struct ShortCounter(u32);

    impl ShortCounter {
        pub(super) fn zero() -> Self {
            Self(0)
        }

        pub(super) fn one() -> Self {
            Self(1)
        }

        pub(super) fn is_one(&self) -> bool {
            self.0 == 1
        }

        pub(super) fn is_zero(&self) -> bool {
            self.0 == 0
        }

        pub(super) fn current(&self) -> u32 {
            self.0
        }

        #[must_use]
        pub(super) fn increment(self) -> Self {
            Self(self.0.checked_add(1).unwrap())
        }

        #[must_use]
        pub(super) fn decrement(self) -> Self {
            Self(self.0.checked_sub(1).unwrap())
        }

        pub(super) fn increment_self(&mut self) {
            *self = self.increment()
        }

        pub(super) fn decrement_self(&mut self) -> &mut Self {
            *self = self.decrement();
            self
        }
    }

    #[derive(Debug, Default)]
    pub(super) struct TokenCell<V>(UnsafeCell<V>);

    impl<V> TokenCell<V> {
        // non-const to forbid unprotected sharing via static variables among threads.
        pub(super) fn new(value: V) -> Self {
            Self(UnsafeCell::new(value))
        }

        pub(super) fn borrow_mut<'t, F>(&self, _token: &'t mut Token<V, F>) -> &'t mut F
        where
            Token<V, F>: TokenTrait<V, F>,
        {
            Token::partial_borrow_mut(unsafe { &mut *self.0.get() })
        }

        pub(super) fn borrow<'t, F>(&self, _token: &'t Token<V, F>) -> &'t F
        where
            Token<V, F>: TokenTrait<V, F>,
        {
            Token::partial_borrow(unsafe { &*self.0.get() })
        }
    }

    unsafe impl<V> Send for TokenCell<V> {}
    unsafe impl<V> Sync for TokenCell<V> {}

    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
    pub(super) struct Token<V, F>(PhantomData<(*mut V, *mut F)>);

    pub trait TokenTrait<V, F> {
        fn partial_borrow(v: &V) -> &F;

        fn partial_borrow_mut(v: &mut V) -> &mut F;
    }

    impl<V, F> Token<V, F> {
        pub(super) unsafe fn assume_on_the_scheduler_thread() -> Self {
            Self(PhantomData)
        }
    }

    // generic identity conversion impl
    impl<T> TokenTrait<T, T> for Token<T, T> {
        fn partial_borrow(t: &T) -> &T {
            t
        }

        fn partial_borrow_mut(t: &mut T) -> &mut T {
            t
        }
    }
}

#[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
#[derive(Debug)]
struct TaskStatus {
    lock_attempts: Vec<LockAttempt>,
    blocked_lock_count: ShortCounter,
}

impl TokenTrait<TaskStatus, ShortCounter> for Token<TaskStatus, ShortCounter> {
    fn partial_borrow(v: &TaskStatus) -> &ShortCounter {
        &v.blocked_lock_count
    }

    fn partial_borrow_mut(v: &mut TaskStatus) -> &mut ShortCounter {
        &mut v.blocked_lock_count
    }
}

impl TokenTrait<TaskStatus, Vec<LockAttempt>> for Token<TaskStatus, Vec<LockAttempt>> {
    fn partial_borrow(v: &TaskStatus) -> &Vec<LockAttempt> {
        &v.lock_attempts
    }

    fn partial_borrow_mut(v: &mut TaskStatus) -> &mut Vec<LockAttempt> {
        &mut v.lock_attempts
    }
}

type PageToken = Token<PageInner, PageInner>;
const_assert_eq!(mem::size_of::<PageToken>(), 0);

type BlockedLockCountToken = Token<TaskStatus, ShortCounter>;
const_assert_eq!(mem::size_of::<BlockedLockCountToken>(), 0);

type LockAttemptToken = Token<TaskStatus, Vec<LockAttempt>>;
const_assert_eq!(mem::size_of::<LockAttemptToken>(), 0);

impl TaskStatus {
    fn new(lock_attempts: Vec<LockAttempt>) -> Self {
        Self {
            lock_attempts,
            blocked_lock_count: ShortCounter::zero(),
        }
    }
}

#[cfg_attr(
    feature = "dev-context-only-utils",
    field_qualifiers(unique_weight(pub))
)]
#[derive(Debug)]
pub struct TaskInner {
    // put this field out of this struct for maximum space efficiency?
    unique_weight: UniqueWeight,
    transaction: SanitizedTransaction, // actually should be Bundle
    task_status: TokenCell<TaskStatus>,
}

impl TaskInner {
    pub fn transaction(&self) -> &SanitizedTransaction {
        &self.transaction
    }

    fn lock_attempts_mut<'t>(
        &self,
        lock_attempt_token: &'t mut LockAttemptToken,
    ) -> &'t mut Vec<LockAttempt> {
        self.task_status.borrow_mut(lock_attempt_token)
    }

    fn blocked_lock_count_mut<'t>(
        &self,
        blocked_lock_count_token: &'t mut BlockedLockCountToken,
    ) -> &'t mut ShortCounter {
        self.task_status.borrow_mut(blocked_lock_count_token)
    }

    fn lock_attempts<'t>(&self, lock_attempt_token: &'t LockAttemptToken) -> &'t Vec<LockAttempt> {
        self.task_status.borrow(lock_attempt_token)
    }

    pub fn task_index(&self) -> usize {
        UniqueWeight::max_value()
            .checked_sub(self.unique_weight)
            .unwrap() as usize
    }
}

#[derive(Debug)]
struct LockAttempt {
    page: Page,
    requested_usage: RequestedUsage,
    lock_status: LockStatus,
}
const_assert_eq!(mem::size_of::<LockAttempt>(), 24);

impl LockAttempt {
    fn new(page: Page, requested_usage: RequestedUsage) -> Self {
        Self {
            page,
            requested_usage,
            lock_status: LockStatus::default(),
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
    Readonly(ShortCounter),
    Writable,
}
const_assert_eq!(mem::size_of::<Usage>(), 8);

impl Usage {
    fn renew(requested_usage: RequestedUsage) -> Self {
        match requested_usage {
            RequestedUsage::Readonly => Usage::Readonly(ShortCounter::one()),
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
    usage: Usage,
    blocked_tasks: BTreeMap<UniqueWeight, (Task, RequestedUsage)>,
}

impl PageInner {
    fn insert_blocked_task(&mut self, task: Task, requested_usage: RequestedUsage) {
        let pre_existed = self
            .blocked_tasks
            .insert(task.unique_weight, (task, requested_usage));
        assert!(pre_existed.is_none());
    }

    fn no_blocked_tasks(&self) -> bool {
        self.blocked_tasks.is_empty()
    }

    fn pop_blocked_task(&mut self, unique_weight: UniqueWeight) {
        let (pre_existed, _) = self.blocked_tasks.pop_last().unwrap();
        assert_eq!(pre_existed, unique_weight);
    }

    fn heaviest_blocked_task(&self) -> Option<&(Task, RequestedUsage)> {
        self.blocked_tasks.last_key_value().map(|(_weight, v)| v)
    }
}

const_assert_eq!(mem::size_of::<TokenCell<PageInner>>(), 32);

// very opaque wrapper type; no methods just with .clone() and ::default()
#[derive(Debug, Clone, Default)]
pub struct Page(Arc<TokenCell<PageInner>>);
const_assert_eq!(mem::size_of::<Page>(), 8);

#[cfg_attr(
    feature = "dev-context-only-utils",
    field_qualifiers(blocked_lock_count_token(pub))
)]
pub struct SchedulingStateMachine {
    unblocked_task_queue: VecDeque<Task>,
    active_task_count: ShortCounter,
    handled_task_count: ShortCounter,
    unblocked_task_count: ShortCounter,
    total_task_count: ShortCounter,
    blocked_lock_count_token: BlockedLockCountToken,
    lock_attempt_token: LockAttemptToken,
    page_token: PageToken,
}
const_assert_eq!(mem::size_of::<SchedulingStateMachine>(), 48);

impl SchedulingStateMachine {
    pub fn is_empty(&self) -> bool {
        self.active_task_count.is_zero()
    }

    pub fn unblocked_task_queue_count(&self) -> usize {
        self.unblocked_task_queue.len()
    }

    pub fn active_task_count(&self) -> u32 {
        self.active_task_count.current()
    }

    pub fn handled_task_count(&self) -> u32 {
        self.handled_task_count.current()
    }

    pub fn unblocked_task_count(&self) -> u32 {
        self.unblocked_task_count.current()
    }

    pub fn total_task_count(&self) -> u32 {
        self.total_task_count.current()
    }

    #[cfg(feature = "dev-context-only-utils")]
    pub fn schedule_task_for_test(&mut self, task: Task) -> Option<Task> {
        self.schedule_task(task, |task| task.clone())
    }

    pub fn schedule_task<R>(
        &mut self,
        task: Task,
        on_success: impl FnOnce(&Task) -> R,
    ) -> Option<R> {
        let ret = self.try_lock_for_task(task, on_success);
        self.total_task_count.increment_self();
        self.active_task_count.increment_self();
        ret
    }

    pub fn has_unblocked_task(&self) -> bool {
        !self.unblocked_task_queue.is_empty()
    }

    #[cfg(feature = "dev-context-only-utils")]
    pub fn schedule_unblocked_task_for_test(&mut self) -> Option<Task> {
        self.schedule_unblocked_task(|task| task.clone())
    }

    pub fn schedule_unblocked_task<R>(&mut self, on_success: impl FnOnce(&Task) -> R) -> Option<R> {
        self.unblocked_task_queue.pop_front().map(|task| {
            let ret = on_success(&task);
            self.unblocked_task_count.increment_self();
            ret
        })
    }

    pub fn deschedule_task(&mut self, task: &Task) {
        self.active_task_count.decrement_self();
        self.handled_task_count.increment_self();
        self.unlock_after_execution(task);
    }

    fn attempt_lock_for_execution(
        page_token: &mut PageToken,
        lock_attempts: &mut [LockAttempt],
    ) -> ShortCounter {
        let mut lock_count = ShortCounter::zero();

        for attempt in lock_attempts.iter_mut() {
            let page = attempt.page_mut(page_token);
            let lock_status = if page.no_blocked_tasks() {
                Self::attempt_lock_address(page, attempt.requested_usage)
            } else {
                LockStatus::Failed
            };
            match lock_status {
                LockStatus::Succeded(usage) => {
                    page.usage = usage;
                }
                LockStatus::Failed => {
                    lock_count.increment_self();
                }
            }
            attempt.lock_status = lock_status;
        }

        lock_count
    }

    fn attempt_lock_address(page: &PageInner, requested_usage: RequestedUsage) -> LockStatus {
        match page.usage {
            Usage::Unused => LockStatus::Succeded(Usage::renew(requested_usage)),
            Usage::Readonly(count) => match requested_usage {
                RequestedUsage::Readonly => {
                    LockStatus::Succeded(Usage::Readonly(count.increment()))
                }
                RequestedUsage::Writable => LockStatus::Failed,
            },
            Usage::Writable => LockStatus::Failed,
        }
    }

    fn unlock(page_token: &mut PageToken, attempt: &LockAttempt) -> bool {
        let mut is_unused_now = false;

        let requested_usage = attempt.requested_usage;
        let page = attempt.page_mut(page_token);

        match &mut page.usage {
            Usage::Readonly(ref mut count) => match requested_usage {
                RequestedUsage::Readonly => {
                    if count.is_one() {
                        is_unused_now = true;
                    } else {
                        count.decrement_self();
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
            page.usage = Usage::Unused;
        }

        is_unused_now
    }

    fn try_lock_for_task<R>(
        &mut self,
        task: Task,
        on_success: impl FnOnce(&Task) -> R,
    ) -> Option<R> {
        let blocked_lock_count = Self::attempt_lock_for_execution(
            &mut self.page_token,
            task.lock_attempts_mut(&mut self.lock_attempt_token),
        );

        //eprintln!("{:?}", blocked_lock_count);
        if blocked_lock_count.current() > 0 {
            *task.blocked_lock_count_mut(&mut self.blocked_lock_count_token) = blocked_lock_count;
            self.register_blocked_task_into_pages(&task);
            None
        } else {
            Some(on_success(&task))
        }
    }

    fn register_blocked_task_into_pages(&mut self, task: &Task) {
        for lock_attempt in task.lock_attempts_mut(&mut self.lock_attempt_token) {
            if matches!(lock_attempt.lock_status, LockStatus::Failed) {
                let requested_usage = lock_attempt.requested_usage;
                lock_attempt
                    .page_mut(&mut self.page_token)
                    .insert_blocked_task(task.clone(), requested_usage);
            }
        }
    }

    fn unlock_after_execution(&mut self, task: &Task) {
        for unlock_attempt in task.lock_attempts(&self.lock_attempt_token) {
            let is_unused_now = Self::unlock(&mut self.page_token, unlock_attempt);
            if !is_unused_now {
                continue;
            }

            loop {
                let page = unlock_attempt.page_mut(&mut self.page_token);
                let heaviest_uncontended_now = page.heaviest_blocked_task();
                let mut should_continue = false;
                if let Some(&(ref uncontended_task, requested_usage)) = heaviest_uncontended_now {
                    let new_count = uncontended_task
                        .blocked_lock_count_mut(&mut self.blocked_lock_count_token)
                        .decrement_self()
                        .current();
                    if new_count == 0 {
                        self.unblocked_task_queue
                            .push_back(uncontended_task.clone());
                    }
                    page.pop_blocked_task(uncontended_task.unique_weight);
                    match Self::attempt_lock_address(page, requested_usage) {
                        LockStatus::Failed | LockStatus::Succeded(Usage::Unused) => unreachable!(),
                        LockStatus::Succeded(usage) => {
                            if matches!(usage, Usage::Readonly(_))
                                && matches!(
                                    page.heaviest_blocked_task(),
                                    Some((_, RequestedUsage::Readonly))
                                )
                            {
                                should_continue = true;
                            }
                            page.usage = usage;
                        }
                    }
                }
                if should_continue {
                    continue;
                } else {
                    break;
                }
            }
        }
    }

    pub fn create_task(
        transaction: SanitizedTransaction,
        index: usize,
        page_loader: &mut impl FnMut(Pubkey) -> Page,
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

        let unique_weight = UniqueWeight::max_value()
            .checked_sub(index as UniqueWeight)
            .unwrap();

        Task::new(TaskInner {
            unique_weight,
            transaction,
            task_status: TokenCell::new(TaskStatus::new(locks)),
        })
    }
}

impl Default for SchedulingStateMachine {
    fn default() -> Self {
        Self {
            unblocked_task_queue: VecDeque::with_capacity(1024),
            active_task_count: ShortCounter::zero(),
            handled_task_count: ShortCounter::zero(),
            unblocked_task_count: ShortCounter::zero(),
            total_task_count: ShortCounter::zero(),
            blocked_lock_count_token: unsafe {
                BlockedLockCountToken::assume_on_the_scheduler_thread()
            },
            lock_attempt_token: unsafe { LockAttemptToken::assume_on_the_scheduler_thread() },
            page_token: unsafe { PageToken::assume_on_the_scheduler_thread() },
        }
    }
}

type UniqueWeight = u64;

#[cfg(test)]
mod tests {
    use {
        super::*,
        assert_matches::assert_matches,
        solana_sdk::{
            instruction::{AccountMeta, Instruction},
            message::Message,
            pubkey::Pubkey,
            signature::Signer,
            signer::keypair::Keypair,
            transaction::{SanitizedTransaction, Transaction},
        },
        std::{collections::HashMap, sync::Mutex},
    };

    fn simplest_transaction() -> SanitizedTransaction {
        let payer = Keypair::new();
        let message = Message::new(&[], Some(&payer.pubkey()));
        let unsigned = Transaction::new_unsigned(message);
        SanitizedTransaction::from_transaction_for_tests(unsigned)
    }

    fn readonly_transaction(address: Pubkey) -> SanitizedTransaction {
        let instruction = Instruction {
            program_id: Pubkey::default(),
            accounts: vec![AccountMeta::new_readonly(address, false)],
            data: vec![],
        };
        let message = Message::new(&[instruction], Some(&Pubkey::new_unique()));
        let unsigned = Transaction::new_unsigned(message);
        SanitizedTransaction::from_transaction_for_tests(unsigned)
    }

    fn transaction_with_shared_writable(address: Pubkey) -> SanitizedTransaction {
        let instruction = Instruction {
            program_id: Pubkey::default(),
            accounts: vec![AccountMeta::new(address, false)],
            data: vec![],
        };
        let message = Message::new(&[instruction], Some(&Pubkey::new_unique()));
        let unsigned = Transaction::new_unsigned(message);
        SanitizedTransaction::from_transaction_for_tests(unsigned)
    }

    fn create_address_loader(
        pages: Option<Arc<Mutex<HashMap<Pubkey, Page>>>>,
    ) -> impl FnMut(Pubkey) -> Page {
        let pages = pages.unwrap_or_default();
        move |address| pages.lock().unwrap().entry(address).or_default().clone()
    }

    #[test]
    fn test_debug() {
        // these are almost meaningless just to see eye-pleasing coverage report....
        assert_eq!(
            format!(
                "{:?}",
                LockStatus::Succeded(Usage::Readonly(ShortCounter::one()))
            ),
            "Succeded(Readonly(ShortCounter(1)))"
        );
        let task_status = TaskStatus {
            lock_attempts: vec![LockAttempt::new(Page::default(), RequestedUsage::Writable)],
            blocked_lock_count: ShortCounter::zero(),
        };
        assert_eq!(
            format!("{:?}", task_status),
            "TaskStatus { lock_attempts: [LockAttempt { page: Page(TokenCell(UnsafeCell { \
             .. })), requested_usage: Writable, lock_status: Failed }], blocked_lock_count: \
             ShortCounter(0) }"
        );
        let sanitized = simplest_transaction();
        let task = SchedulingStateMachine::create_task(sanitized, 0, &mut |_| Page::default());
        assert!(format!("{:?}", task).contains("TaskInner"));
    }

    #[test]
    fn test_scheduling_state_machine_default() {
        let state_machine = SchedulingStateMachine::default();
        assert_eq!(state_machine.active_task_count(), 0);
        assert_eq!(state_machine.total_task_count(), 0);
        assert!(state_machine.is_empty());
    }

    #[test]
    fn test_create_task() {
        let sanitized = simplest_transaction();
        let task =
            SchedulingStateMachine::create_task(sanitized.clone(), 3, &mut |_| Page::default());
        assert_eq!(task.task_index(), 3);
        assert_eq!(task.transaction(), &sanitized);
    }

    #[test]
    fn test_schedule_non_conflicting_task() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        let task = state_machine.schedule_task_for_test(task).unwrap();
        assert_eq!(state_machine.active_task_count(), 1);
        assert_eq!(state_machine.total_task_count(), 1);
        state_machine.deschedule_task(&task);
        assert_eq!(state_machine.active_task_count(), 0);
        assert_eq!(state_machine.total_task_count(), 1);
        drop(task);
    }

    #[test]
    fn test_schedule_conflicting_task() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized.clone(), 5, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(state_machine.schedule_task_for_test(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task_for_test(task2.clone()), None);

        state_machine.deschedule_task(&task1);
        assert!(state_machine.has_unblocked_task());
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);
        assert_eq!(
            state_machine
                .schedule_unblocked_task_for_test()
                .unwrap()
                .task_index(),
            task2.task_index()
        );
        assert!(!state_machine.has_unblocked_task());
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task2);

        assert_matches!(state_machine.schedule_task_for_test(task3.clone()), Some(_));
    }

    #[test]
    fn test_schedule_unblocked_task() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 4, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(state_machine.schedule_task_for_test(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task_for_test(task2.clone()), None);

        state_machine.deschedule_task(&task1);

        assert_eq!(state_machine.unblocked_task_count(), 0);
        assert_eq!(
            state_machine
                .schedule_unblocked_task_for_test()
                .unwrap()
                .task_index(),
            task2.task_index()
        );
        assert_eq!(state_machine.unblocked_task_count(), 1);
        assert_matches!(state_machine.schedule_unblocked_task_for_test(), None);
        assert_eq!(state_machine.unblocked_task_count(), 1);
    }

    #[test]
    fn test_schedule_unblocked_task2() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized.clone(), 5, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(state_machine.schedule_task_for_test(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task_for_test(task2.clone()), None);

        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);

        assert_matches!(state_machine.schedule_task_for_test(task3.clone()), None);

        assert_eq!(state_machine.unblocked_task_count(), 0);
        assert_matches!(state_machine.schedule_unblocked_task_for_test(), Some(_));
        assert_eq!(state_machine.unblocked_task_count(), 1);
        assert_matches!(state_machine.schedule_unblocked_task_for_test(), None);
        assert_eq!(state_machine.unblocked_task_count(), 1);

        state_machine.deschedule_task(&task2);

        assert_matches!(state_machine.schedule_unblocked_task_for_test(), Some(_));
        assert_eq!(state_machine.unblocked_task_count(), 2);

        state_machine.deschedule_task(&task3);
        assert!(state_machine.is_empty());
    }

    #[test]
    fn test_schedule_unblocked_task3() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized.clone(), 5, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(state_machine.schedule_task_for_test(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task_for_test(task2.clone()), None);

        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);

        assert_matches!(state_machine.schedule_task_for_test(task3.clone()), None);
    }

    #[test]
    fn test_schedule_multiple_readonly_task() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = readonly_transaction(conflicting_address);
        let sanitized2 = readonly_transaction(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(state_machine.schedule_task_for_test(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task_for_test(task2.clone()), Some(_));

        assert_eq!(state_machine.active_task_count(), 2);
        assert_eq!(state_machine.handled_task_count(), 0);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.active_task_count(), 1);
        assert_eq!(state_machine.handled_task_count(), 1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task2);
        assert_eq!(state_machine.active_task_count(), 0);
        assert_eq!(state_machine.handled_task_count(), 2);
    }

    #[test]
    fn test_schedule_multiple_writable_tasks() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = readonly_transaction(conflicting_address);
        let sanitized2 = readonly_transaction(conflicting_address);
        let sanitized3 = transaction_with_shared_writable(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized3, 5, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(
            state_machine
                .schedule_task_for_test(task1.clone())
                .map(|t| t.task_index()),
            Some(3)
        );
        assert_matches!(
            state_machine
                .schedule_task_for_test(task2.clone())
                .map(|t| t.task_index()),
            Some(4)
        );
        assert_matches!(state_machine.schedule_task_for_test(task3.clone()), None);

        assert_eq!(state_machine.active_task_count(), 3);
        assert_eq!(state_machine.handled_task_count(), 0);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.active_task_count(), 2);
        assert_eq!(state_machine.handled_task_count(), 1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task2);
        assert_eq!(state_machine.active_task_count(), 1);
        assert_eq!(state_machine.handled_task_count(), 2);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);
    }

    #[test]
    fn test_schedule_rw_mixed() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = readonly_transaction(conflicting_address);
        let sanitized2 = transaction_with_shared_writable(conflicting_address);
        let sanitized3 = readonly_transaction(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized3, 5, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(
            state_machine
                .schedule_task_for_test(task1.clone())
                .map(|t| t.task_index()),
            Some(3)
        );
        assert_matches!(state_machine.schedule_task_for_test(task2.clone()), None);
        assert_matches!(state_machine.schedule_task_for_test(task3.clone()), None);

        assert_eq!(state_machine.active_task_count(), 3);
        assert_eq!(state_machine.handled_task_count(), 0);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.active_task_count(), 2);
        assert_eq!(state_machine.handled_task_count(), 1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);
        assert_matches!(
            state_machine
                .schedule_unblocked_task_for_test()
                .map(|t| t.task_index()),
            Some(4)
        );
    }

    #[test]
    fn test_schedule_writable_after_readonly() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = readonly_transaction(conflicting_address);
        let sanitized2 = transaction_with_shared_writable(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(
            state_machine
                .schedule_task_for_test(task1.clone())
                .map(|t| t.task_index()),
            Some(3)
        );
        assert_matches!(state_machine.schedule_task_for_test(task2.clone()), None);

        state_machine.deschedule_task(&task1);
        assert_matches!(
            state_machine
                .schedule_unblocked_task_for_test()
                .map(|t| t.task_index()),
            Some(4)
        );
        state_machine.deschedule_task(&task2);
    }

    #[test]
    fn test_schedule_readonly_after_writable() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_shared_writable(conflicting_address);
        let sanitized2 = readonly_transaction(conflicting_address);
        let sanitized3 = readonly_transaction(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized3, 5, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(state_machine.schedule_task_for_test(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task_for_test(task2.clone()), None);
        assert_matches!(state_machine.schedule_task_for_test(task3.clone()), None);

        state_machine.deschedule_task(&task1);
        assert_matches!(
            state_machine
                .schedule_unblocked_task_for_test()
                .map(|t| t.task_index()),
            Some(4)
        );
        assert_matches!(
            state_machine
                .schedule_unblocked_task_for_test()
                .map(|t| t.task_index()),
            Some(5)
        );
    }

    #[test]
    fn test_rollback() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_shared_writable(conflicting_address);
        let sanitized2 = transaction_with_shared_writable(conflicting_address);
        let pages = Arc::new(Mutex::new(HashMap::new()));
        let address_loader = &mut create_address_loader(Some(pages.clone()));
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);

        let mut state_machine = SchedulingStateMachine::default();
        assert_matches!(state_machine.schedule_task_for_test(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task_for_test(task2.clone()), None);
        let pages = pages.lock().unwrap();
        let page = pages.get(&conflicting_address).unwrap();
        assert_matches!(
            page.0.borrow(&state_machine.page_token).usage,
            Usage::Writable
        );
        let page = pages
            .get(task2.transaction().message().fee_payer())
            .unwrap();
        assert_matches!(
            page.0.borrow(&state_machine.page_token).usage,
            Usage::Writable
        );
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn test_unreachable_unlock_conditions() {
        let mut state_machine = SchedulingStateMachine::default();
        SchedulingStateMachine::unlock(
            &mut state_machine.page_token,
            &LockAttempt::new(Page::default(), RequestedUsage::Writable),
        );
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn test_unreachable_unlock_conditions2() {
        let mut state_machine = SchedulingStateMachine::default();
        let page = Page::default();
        page.0.borrow_mut(&mut state_machine.page_token).usage = Usage::Writable;
        SchedulingStateMachine::unlock(
            &mut state_machine.page_token,
            &LockAttempt::new(page, RequestedUsage::Readonly),
        );
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn test_unreachable_unlock_conditions3() {
        let mut state_machine = SchedulingStateMachine::default();
        let page = Page::default();
        page.0.borrow_mut(&mut state_machine.page_token).usage =
            Usage::Readonly(ShortCounter::one());
        SchedulingStateMachine::unlock(
            &mut state_machine.page_token,
            &LockAttempt::new(page, RequestedUsage::Writable),
        );
    }
}
